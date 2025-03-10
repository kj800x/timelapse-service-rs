use chrono::{DateTime, Utc};
use maud::{html, Markup};
use poem::http::StatusCode;
use poem::listener::TcpListener;
use poem::web::{Data, Path, Query};
use poem::IntoResponse;
use poem::{get, handler, EndpointExt, Route, Server};
use poem_openapi::payload::Binary;
use serde::Deserialize;
use std::fmt::{self, Display, Formatter};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::{env, fs};
use tempfile::NamedTempFile;

#[derive(Clone)]
struct CommaSeparatedString(Vec<String>);

impl<'de> Deserialize<'de> for CommaSeparatedString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s: String = Deserialize::deserialize(deserializer)?;
        Ok(CommaSeparatedString(
            s.split(',').map(|s| s.to_string()).collect(),
        ))
    }
}

impl From<CommaSeparatedString> for Vec<String> {
    fn from(value: CommaSeparatedString) -> Self {
        value.0
    }
}

#[derive(Deserialize)]
struct QueryParams {
    fps: Option<usize>,
    ffmpeg_args: Option<CommaSeparatedString>,
}

#[derive(Debug, Clone)]
struct Frame {
    path: PathBuf,
    timestamp: i64,
}

#[derive(Debug)]
struct FrameCollection {
    frames: Vec<Frame>,
}

impl FrameCollection {
    fn new(folder: PathBuf) -> Self {
        let read_dir = fs::read_dir(&folder).unwrap();

        let frames: Vec<Frame> = read_dir
            .filter_map(|entry| {
                let entry = entry.unwrap();
                let file_name = entry.file_name().into_string().unwrap();
                let file_name_without_extension = file_name.trim_end_matches(".jpg"); // Adjust the extension if needed
                let timestamp: Result<i64, _> = file_name_without_extension.parse();

                match timestamp {
                    Ok(timestamp) => Some(Frame {
                        path: entry.path(),
                        timestamp,
                    }),
                    Err(_) => None,
                }
            })
            .collect();

        FrameCollection { frames }
    }

    fn get_range(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Self {
        let mut frames: Vec<Frame> = self
            .frames
            .iter()
            .filter(|frame| {
                frame.timestamp > start.timestamp() && frame.timestamp < end.timestamp()
            })
            .map(|frame| frame.clone())
            .collect();

        println!(
            "Found {} frames between {} and {}",
            frames.len(),
            start.format("%Y-%m-%d %H:%M:%S UTC"),
            end.format("%Y-%m-%d %H:%M:%S UTC")
        );
        frames.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

        FrameCollection { frames }
    }

    fn get_past_days(&self, days: i64) -> Self {
        let now = Utc::now();
        let days_ago = now - chrono::Duration::days(days);

        self.get_range(days_ago, now)
    }

    fn into_paths(self) -> Vec<PathBuf> {
        self.frames.into_iter().map(|frame| frame.path).collect()
    }

    fn into_mp4(
        self,
        fps: usize,
        args_override: Option<Vec<String>>,
    ) -> poem::Result<poem::Response> {
        if self.frames.len() == 0 {
            return Ok(poem::Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(()));
        }

        let temp_file = NamedTempFile::new().expect("Failed to create temporary file");
        let temp_path = temp_file.path().to_str().unwrap().to_string();

        let mut child = Command::new("ffmpeg")
            .args(args_override.unwrap_or_else(|| {
                vec![
                    "-y".to_string(),
                    "-safe".to_string(),
                    "0".to_string(),
                    "-protocol_whitelist".to_string(),
                    "pipe,file".to_string(),
                    "-f".to_string(),
                    "concat".to_string(),
                    "-i".to_string(),
                    "pipe:0".to_string(),
                    "-c:v".to_string(),
                    "libx264".to_string(),
                    "-preset".to_string(),
                    "ultrafast".to_string(),
                    "-crf".to_string(),
                    "18".to_string(),
                    "-movflags".to_string(),
                    "+faststart".to_string(),
                    "-f".to_string(),
                    "mp4".to_string(),
                    temp_path.to_string(),
                ]
            }))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("Failed to spawn child process");

        let mut stdin = child.stdin.take().expect("Failed to open stdin");
        let mut ffmpeg_input = String::new();
        for path in self.into_paths() {
            ffmpeg_input.push_str(&format!("file 'file:{}'\n", path.to_str().unwrap()));
            ffmpeg_input.push_str(&format!("outpoint {:.2}\n", 1f32 / fps as f32));
        }

        std::thread::spawn(move || {
            stdin
                .write_all(ffmpeg_input.as_bytes())
                .expect("Failed to write to stdin");
        });

        let output = child.wait_with_output().expect("Failed to read stdout");

        // Only show FFmpeg output if there was an error
        if !output.status.success() {
            eprintln!("FFmpeg failed with status: {}", output.status);
            if !output.stderr.is_empty() {
                eprintln!("FFmpeg error: {}", String::from_utf8_lossy(&output.stderr));
            }
            return Ok(poem::Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body("ffmpeg failed to create video"));
        }

        // Read the temporary file into memory
        let video_data = match fs::read(&temp_path) {
            Ok(data) => data,
            Err(e) => {
                eprintln!("Failed to read temporary file: {}", e);
                return Ok(poem::Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body("failed to read output video"));
            }
        };

        println!(
            "Successfully created {:.1}MB video",
            video_data.len() as f64 / 1_048_576.0
        );

        Ok(<Vec<u8> as Clone>::clone(&Binary(video_data.clone()))
            .with_content_type("video/mp4")
            .into_response())
    }
}

#[derive(Clone)]
struct FrameFolder(String);

impl Display for FrameFolder {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[handler]
fn week_handler(
    Path(folder): Path<String>,
    Data(FrameFolder(frame_folder)): Data<&FrameFolder>,
    params: Query<QueryParams>,
) -> poem::Result<poem::Response> {
    let resolved_folder = PathBuf::from(frame_folder).join(folder);
    let frame_collection = FrameCollection::new(resolved_folder);

    frame_collection.get_past_days(7).into_mp4(
        params.fps.unwrap_or(20),
        params.ffmpeg_args.as_ref().map(|x| x.clone().into()),
    )
}

#[handler]
fn forty_eight_handler(
    Path(folder): Path<String>,
    Data(FrameFolder(frame_folder)): Data<&FrameFolder>,
    params: Query<QueryParams>,
) -> poem::Result<poem::Response> {
    let resolved_folder = PathBuf::from(frame_folder).join(folder);
    let frame_collection = FrameCollection::new(resolved_folder);

    frame_collection.get_past_days(2).into_mp4(
        params.fps.unwrap_or(20),
        params.ffmpeg_args.as_ref().map(|x| x.clone().into()),
    )
}

#[handler]
fn twenty_four_handler(
    Path(folder): Path<String>,
    Data(FrameFolder(frame_folder)): Data<&FrameFolder>,
    params: Query<QueryParams>,
) -> poem::Result<poem::Response> {
    let resolved_folder = PathBuf::from(frame_folder).join(folder);
    let frame_collection = FrameCollection::new(resolved_folder);

    frame_collection.get_past_days(1).into_mp4(
        params.fps.unwrap_or(20),
        params.ffmpeg_args.as_ref().map(|x| x.clone().into()),
    )
}

#[handler]
fn day_handler(
    Path((day, folder)): Path<(String, String)>,
    Data(FrameFolder(frame_folder)): Data<&FrameFolder>,
    params: Query<QueryParams>,
) -> poem::Result<poem::Response> {
    let resolved_folder = PathBuf::from(frame_folder).join(folder);
    let frame_collection = FrameCollection::new(resolved_folder);

    // Assume the day is in the format YYYY-MM-DD and the timezone is Eastern
    // TODO: what do we do for DST?
    let start = format!("{}T00:00:00-04:00", day);
    let end = format!("{}T23:59:59-04:00", day);
    let start = DateTime::parse_from_rfc3339(&start).unwrap();
    let end = DateTime::parse_from_rfc3339(&end).unwrap();

    frame_collection
        .get_range(start.into(), end.into())
        .into_mp4(
            params.fps.unwrap_or(20),
            params.ffmpeg_args.as_ref().map(|x| x.clone().into()),
        )
}

#[handler]
fn exact_handler(
    Path((start, end, folder)): Path<(String, String, String)>,
    Data(FrameFolder(frame_folder)): Data<&FrameFolder>,
    params: Query<QueryParams>,
) -> poem::Result<poem::Response> {
    let resolved_folder = PathBuf::from(frame_folder).join(folder);
    let frame_collection = FrameCollection::new(resolved_folder);

    let start = DateTime::parse_from_rfc3339(&start).unwrap();
    let end = DateTime::parse_from_rfc3339(&end).unwrap();

    frame_collection
        .get_range(start.into(), end.into())
        .into_mp4(
            params.fps.unwrap_or(20),
            params.ffmpeg_args.as_ref().map(|x| x.clone().into()),
        )
}

#[handler]
fn timelapse_index_handler(Data(FrameFolder(frame_folder)): Data<&FrameFolder>) -> Markup {
    // Read the files in the folder
    let folders: Vec<String> = fs::read_dir(&frame_folder)
        .unwrap()
        .filter_map(|entry| {
            let entry = entry.unwrap();
            let file_name = entry.file_name().into_string().unwrap();
            if entry.file_type().unwrap().is_dir() {
                Some(file_name)
            } else {
                None
            }
        })
        .collect();

    html! {
        style {
            "body { font-family: sans-serif; color: white; background-color: #333; }"
            "h1, h2 { color: #f90; }"
            "h3 { color: #f90; }"
            "a { color: #f90; text-decoration: none; }"
            "a:hover { text-decoration: underline; }"
            "p { margin-bottom: 1em; }"
            "body { max-width: 800px; margin: 0 auto; padding: 1em; }"s
            "h1 { font-size: 2em; }"
            "h2 { font-size: 1.5em; }"
            "h3 { font-size: 1.2em; }"
            "ul { list-style-type: none; }"
            "li { margin-bottom: 1em; }"
        }
        h1 { "Timelapse API" }
        p { "This API generates timelapse videos from a folder of images." }
        h2 { "Folders" }
        ul {
            @for folder in folders {
                h3 {(folder)}
                ul {
                    li { a href=(format!("/timelapse/24/{}", folder)) { "24 hours" } }
                    li { a href=(format!("/timelapse/48/{}", folder)) { "48 hours" } }
                    li { a href=(format!("/timelapse/1w/{}", folder)) { "1 week" } }
                    li { a href=(format!("/timelapse/day/YYYY-MM-DD/{}", folder)) { "Specific day" } " (invalid link)" }
                    li { a href=(format!("/timelapse/from/[ISO8601]/to/[ISO8601]/{}", folder)) { "Specific range" } " (invalid link)" }
                }
            }
        }
        h2 { "Endpoints" }
        ul {
            li { pre { "GET /timelapse/24/:folder" } }
            li { pre { "GET /timelapse/48/:folder"}  }
            li { pre { "GET /timelapse/1w/:folder" } }
            li { pre { "GET /timelapse/day/YYYY-MM-DD/:folder" } }
            li { pre { "GET /timelapse/from/[ISO8601]/to/[ISO8601]/:folder" } }
        }
    }
}

#[handler]
fn index_redirect_handler() -> impl IntoResponse {
    poem::Response::builder()
        .status(StatusCode::MOVED_PERMANENTLY)
        .header("Location", "/timelapse/")
        .body(())
}

#[handler]
fn healthcheck() -> impl IntoResponse {
    poem::Response::builder().status(StatusCode::OK).body("OK")
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let host = "0.0.0.0";
    let port: i32 = env::var("PORT").map(|x| x.parse().unwrap()).unwrap_or(8102);
    let frame_folder =
        FrameFolder(env::var("OUTPUT_FOLDER").expect("OUTPUT_FOLDER env var required"));
    println!(
        "OUTPUT_FOLDER: {}\nPort: {}\nHost: {}",
        frame_folder, port, host
    );
    println!("http://{}:{}/timelapse/24/:folder", host, port);
    println!("http://{}:{}/timelapse/48/:folder", host, port);
    println!("http://{}:{}/timelapse/1w/:folder", host, port);
    println!("http://{}:{}/timelapse/day/YYYY-MM-DD/:folder", host, port);
    println!(
        "http://{}:{}/timelapse/from/[ISO8601]/to/[ISO8601]/:folder",
        host, port
    );
    let twenty_four_service = Route::new().at("/:folder", get(twenty_four_handler));
    let forty_eight_service = Route::new().at("/:folder", get(forty_eight_handler));
    let week_service = Route::new().at("/:folder", get(week_handler));
    let day_service = Route::new().at("/:day/:folder", get(day_handler));
    let exact_service = Route::new().at("/:start/to/:end/:folder", get(exact_handler));

    let route = Route::new()
        .nest("/timelapse/24", twenty_four_service)
        .nest("/timelapse/48", forty_eight_service)
        .nest("/timelapse/1w", week_service)
        .nest("/timelapse/day", day_service)
        .nest("/timelapse/from", exact_service)
        .at("/timelapse/", get(timelapse_index_handler))
        .at("/timelapse", get(timelapse_index_handler))
        .at("/healthcheck", get(healthcheck))
        .at("/", get(index_redirect_handler))
        .data(frame_folder);
    Server::new(TcpListener::bind(format!("{host}:{port}")))
        .run(route)
        .await?;
    Ok(())
}
