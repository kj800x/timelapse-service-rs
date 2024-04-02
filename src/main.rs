use chrono::Utc;
use engiffen::{engiffen, load_images, Quantizer};
use mime_guess::mime::{self};
use poem::listener::TcpListener;
use poem::web::{Data, Path};
use poem::IntoResponse;
use poem::{get, handler, EndpointExt, Route, Server};
use poem_openapi::payload::Binary;
use std::path::PathBuf;
use std::{env, fs, io::Cursor};

#[derive(Debug, Clone)]
struct Frame {
    path: PathBuf,
    timestamp: u64,
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
                let timestamp: Result<u64, _> = file_name_without_extension.parse();

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

    fn get_today_frames(&self) -> Self {
        let now = Utc::now();
        let one_day_ago = now - chrono::Duration::days(1);

        let mut frames: Vec<Frame> = self
            .frames
            .iter()
            .filter(|frame| frame.timestamp > one_day_ago.timestamp() as u64)
            .map(|frame| frame.clone())
            .collect();

        frames.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

        FrameCollection { frames }
    }

    fn into_paths(self) -> Vec<PathBuf> {
        self.frames.into_iter().map(|frame| frame.path).collect()
    }
}

#[derive(Clone)]
enum FrameFolder {
    FrameFolder(String),
}
impl From<&FrameFolder> for String {
    fn from(value: &FrameFolder) -> Self {
        match value {
            FrameFolder::FrameFolder(x) => x.clone(),
        }
    }
}
#[handler]
fn day_handler(
    path: Path<String>,
    frame_folder: Data<&FrameFolder>,
) -> poem::Result<poem::Response> {
    let resolved_folder = PathBuf::from(String::from(frame_folder.0)).join(path.0);
    let frame_collection = FrameCollection::new(resolved_folder);
    let today_frames = frame_collection.get_today_frames();

    let images = load_images(&today_frames.into_paths());

    // 15 seconds holy heck
    let gif = engiffen(&images, 5, Quantizer::NeuQuant(4)).unwrap();

    let mut buffer = Vec::new();
    gif.write(&mut buffer).unwrap();
    let mut curs = Cursor::new(buffer);

    Ok(Binary(curs.get_ref().clone())
        .with_content_type(mime::IMAGE_GIF.to_string())
        .into_response())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let host = "0.0.0.0";
    let port: i32 = env::var("PORT").map(|x| x.parse().unwrap()).unwrap_or(8102);
    let frame_folder = FrameFolder::FrameFolder(env::var("OUTPUT_FOLDER")?);
    println!(
        "OUTPUT_FOLDER: {}\nPort: {}\nHost: {}",
        String::from(&frame_folder),
        port,
        host
    );
    println!("http://{}:{}/timelapse/day/:folder", host, port);
    let day_service = Route::new().at("/*path", get(day_handler));

    let route = Route::new()
        .nest("/timelapse/day", day_service)
        .data(frame_folder);
    Server::new(TcpListener::bind(format!("{host}:{port}")))
        .run(route)
        .await?;
    Ok(())
}
