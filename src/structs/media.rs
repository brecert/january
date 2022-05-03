use serde::Serialize;

#[derive(Debug, Serialize)]
pub enum ImageSize {
    Large,
    Preview,
}

impl Default for ImageSize {
    fn default() -> Self {
        ImageSize::Preview
    }
}

#[derive(Debug, Serialize, Default)]
pub struct Image {
    pub url: String,
    pub width: isize,
    pub height: isize,
    pub size: ImageSize,
}

#[derive(Debug, Serialize, Default)]
pub struct Video {
    pub url: String,
    pub width: isize,
    pub height: isize,
}
