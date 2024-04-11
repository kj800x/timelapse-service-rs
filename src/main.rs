use chrono::{DateTime, Utc};
use engiffen::{engiffen, load_images, Quantizer};
use mime_guess::mime::{self};
use poem::listener::TcpListener;
use poem::web::{Data, Path, Query};
use poem::IntoResponse;
use poem::{get, handler, EndpointExt, Route, Server};
use poem_openapi::payload::Binary;
use serde::Deserialize;
use std::fmt::{self, Display, Formatter};
use std::path::PathBuf;
use std::{env, fs, io::Cursor};

#[derive(Deserialize)]
struct QueryParams {
    fps: Option<usize>,
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

    fn into_gif(self, fps: usize) -> poem::Result<poem::Response> {
        let images = load_images(&self.into_paths());

        // 15 seconds holy heck
        let gif = engiffen(&images, fps, Quantizer::NeuQuant(4)).unwrap();

        let mut buffer = Vec::new();
        gif.write(&mut buffer).unwrap();
        let curs = Cursor::new(buffer);

        Ok(Binary(curs.get_ref().clone())
            .with_content_type(mime::IMAGE_GIF.to_string())
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

    frame_collection
        .get_past_days(7)
        .into_gif(params.fps.unwrap_or(20))
}

#[handler]
fn forty_eight_handler(
    Path(folder): Path<String>,
    Data(FrameFolder(frame_folder)): Data<&FrameFolder>,
    params: Query<QueryParams>,
) -> poem::Result<poem::Response> {
    let resolved_folder = PathBuf::from(frame_folder).join(folder);
    let frame_collection = FrameCollection::new(resolved_folder);

    frame_collection
        .get_past_days(2)
        .into_gif(params.fps.unwrap_or(20))
}

#[handler]
fn twenty_four_handler(
    Path(folder): Path<String>,
    Data(FrameFolder(frame_folder)): Data<&FrameFolder>,
    params: Query<QueryParams>,
) -> poem::Result<poem::Response> {
    let resolved_folder = PathBuf::from(frame_folder).join(folder);
    let frame_collection = FrameCollection::new(resolved_folder);

    frame_collection
        .get_past_days(1)
        .into_gif(params.fps.unwrap_or(20))
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
        .into_gif(params.fps.unwrap_or(20))
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
        .into_gif(params.fps.unwrap_or(20))
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
        .data(frame_folder);
    Server::new(TcpListener::bind(format!("{host}:{port}")))
        .run(route)
        .await?;
    Ok(())
}
