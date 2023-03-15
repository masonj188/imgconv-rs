#[macro_use]
extern crate rocket;

use std::io::Write;

use image_convert::{to_gif, to_jpg, to_png, GIFConfig, ImageResource, JPGConfig, PNGConfig};
use rocket::form::Form;
use rocket::fs::{relative, FileServer, TempFile};
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
        .mount("/", FileServer::from(relative!("static")))
        .mount("/upload", routes![upload])
}

#[post("/", data = "<file>")]
async fn upload(
    file: Form<Upload<'_>>,
) -> Result<(ContentType, ContentDisposition<Vec<u8>>), BadRequest<&'static str>> {
    let format = Format::try_from(file.format.as_str());
    let format = match format {
        Ok(f) => f,
        Err(_) => return Err(status::BadRequest(Some("unknown format type"))),
    };

    let zip_buf = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(zip_buf);

    let mut handlers: Vec<(JoinHandle<ImageResource>, _)> = Vec::new();

    for i in &file.images {
        let input = ImageResource::from_path(i.path().unwrap());
        let mut output = ImageResource::with_capacity(10000);

        let (handle, filename) = match format {
            Format::PNG => {
                let handle = spawn_blocking(move || {
                    to_png(&mut output, &input, &PNGConfig::default()).unwrap();
                    output
                });
                let filename = format!("{}.png", i.name().unwrap());
                (handle, filename)
            }
            Format::JPEG => {
                let handle = spawn_blocking(move || {
                    to_jpg(&mut output, &input, &JPGConfig::default()).unwrap();
                    output
                });
                let filename = format!("{}.jpeg", i.name().unwrap());
                (handle, filename)
            }
            Format::GIF => {
                let handle = spawn_blocking(move || {
                    to_gif(&mut output, &input, &GIFConfig::default()).unwrap();
                    output
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
        let image_bytes = image_bytes.as_u8_slice().unwrap();
        zip.start_file(
            filename,
            FileOptions::default().compression_method(zip::CompressionMethod::DEFLATE),
        )
        .unwrap();
        zip.write_all(image_bytes).unwrap();
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
