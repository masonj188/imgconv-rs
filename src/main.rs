#[macro_use]
extern crate rocket;

use std::io::{Cursor, Write};

use rocket::form::Form;
use rocket::fs::TempFile;
use rocket::futures::future::join_all;
use rocket::http::{ContentType, Header};
use rocket::response::status::{self, BadRequest};
use rocket::response::Responder;
use rocket::tokio::task::{spawn_blocking, JoinHandle};
use strum::{Display, EnumString};
use zip::write::FileOptions;

#[launch]
fn rocket() -> _ {
    rocket::build()
        .mount("/", routes![index])
        .mount("/upload", routes![upload])
}

#[get("/")]
async fn index() -> (ContentType, &'static str) {
    (ContentType::HTML, include_str!("../static/index.html"))
}

#[post("/", data = "<file>")]
async fn upload<'a>(
    mut file: Form<Upload<'a>>,
) -> Result<(ContentType, ContentDisposition<Vec<u8>>), BadRequest<&'static str>> {
    let format = Format::try_from(file.format.as_str());
    let format = match format {
        Ok(f) => f,
        Err(_) => return Err(status::BadRequest(Some("unknown format type"))),
    };

    let zip_buf = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(zip_buf);

    let mut handlers: Vec<(JoinHandle<Vec<u8>>, _)> = Vec::new();

    for i in &mut file.images {
        let filebuf = rocket::tokio::fs::read(i.path().unwrap()).await.unwrap();
        let guessed_format = image::guess_format(filebuf.as_ref()).unwrap();
        let input = image::load_from_memory_with_format(filebuf.as_ref(), guessed_format).unwrap();
        let mut output = Cursor::new(vec![]);

        let (handle, filename) = match format {
            Format::PNG => {
                let handle = spawn_blocking(move || {
                    input
                        .write_to(&mut output, image::ImageOutputFormat::Png)
                        .unwrap();
                    output.into_inner()
                });
                let filename = format!("{}.png", i.name().unwrap());
                (handle, filename)
            }
            Format::JPEG => {
                let handle = spawn_blocking(move || {
                    input
                        .write_to(&mut output, image::ImageOutputFormat::Jpeg(100))
                        .unwrap();
                    output.into_inner()
                });
                let filename = format!("{}.jpeg", i.name().unwrap());
                (handle, filename)
            }
            Format::GIF => {
                let handle = spawn_blocking(move || {
                    input
                        .write_to(&mut output, image::ImageOutputFormat::Gif)
                        .unwrap();
                    output.into_inner()
                });
                let filename = format!("{}.gif", i.name().unwrap());
                (handle, filename)
            }
        };
        handlers.push((handle, filename));
    }

    let handlers = handlers
        .into_iter()
        .map(|(h, f)| async { (h.await, f) })
        .collect::<Vec<_>>();

    let handlers = join_all(handlers).await;

    for (handle, filename) in handlers {
        let image_bytes = handle.unwrap();
        zip.start_file(
            filename,
            FileOptions::default().compression_method(zip::CompressionMethod::DEFLATE),
        )
        .unwrap();
        zip.write_all(image_bytes.as_ref()).unwrap();
    }

    let zip_buf = zip.finish().unwrap();

    Ok((
        ContentType::ZIP,
        ContentDisposition::new(zip_buf.into_inner(), "attachment; filename=\"images.zip\""),
    ))
}

#[derive(FromForm)]
struct Upload<'a> {
    images: Vec<TempFile<'a>>,
    format: String,
}

#[derive(EnumString, Display)]
enum Format {
    #[strum(ascii_case_insensitive)]
    JPEG,
    #[strum(ascii_case_insensitive)]
    PNG,
    #[strum(ascii_case_insensitive)]
    GIF,
}

#[derive(Responder)]
struct ContentDisposition<T> {
    inner: T,
    value: Header<'static>,
}

impl<T> ContentDisposition<T> {
    fn new(inner: T, value: &str) -> ContentDisposition<T> {
        ContentDisposition {
            inner,
            value: Header::new("content-disposition", value.to_string()),
        }
    }
}
