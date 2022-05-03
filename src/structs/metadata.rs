use regex::Regex;
use reqwest::Response;
use serde::Serialize;

use crate::{
    structs::special::{BandcampType, TwitchType},
    util::{
        request::{consume_size, fetch},
        result::Error,
    },
};

use super::{
    media::{Image, ImageSize, Video},
    special::Special,
};

#[derive(Debug, Serialize, Default)]
pub struct Metadata {
    url: String,
    special: Option<Special>,

    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    image: Option<Image>,
    #[serde(skip_serializing_if = "Option::is_none")]
    video: Option<Video>,

    #[serde(skip_serializing_if = "Option::is_none")]
    opengraph_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    site_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    icon_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    colour: Option<String>,
}

impl Metadata {
    pub async fn from(resp: Response, url: String) -> Result<Metadata, Error> {
        let text = resp.text().await.map_err(|_| Error::FailedToConsumeText)?;
        let mut dom = tl::parse(&text, tl::ParserOptions::default())
            .map_err(|_| Error::FailedToConsumeText)?;

        let mut links = dom
            .nodes()
            .into_iter()
            .filter_map(|node| node.as_tag())
            .filter(|tag| tag.name() == "link")
            .filter_map(|tag| {
                let attributes = tag.attributes();
                let property = attributes.get("rel").flatten();
                let content = attributes.get("href").flatten();
                if let (Some(property), Some(content)) = (property, content) {
                    Some((property, content))
                } else {
                    None
                }
            });

        let url = links
            .find_map(|(name, value)| {
                if name.eq("apple-touch-icon") || name.eq("icon") {
                    Some(value.as_utf8_str().to_string())
                } else {
                    None
                }
            })
            .unwrap_or(url);

        let nodes = dom
            .nodes_mut()
            .into_iter()
            .filter_map(|node| node.as_tag_mut());

        let meta = nodes.filter(|tag| tag.name() == "meta");

        let props = meta.filter_map(|tag| {
            let attrs = tag.attributes_mut();
            attrs
                .remove_value("property")
                .or_else(|| attrs.remove_value("name"))
                .and_then(|name| attrs.remove_value("content").map(|val| (name, val)))
        });

        let mut metadata = Metadata {
            url,
            ..Metadata::default()
        };

        for (name, value) in props {
            match name.as_bytes() {
                b"og:title" | b"twitter:title" | b"title" => {
                    if metadata.title.is_some() {
                        continue;
                    }
                    metadata.title = Some(value.as_utf8_str().to_string())
                }
                b"og:description" | b"twitter:description" | b"description" => {
                    if metadata.description.is_some() {
                        continue;
                    }

                    metadata.description = Some(value.as_utf8_str().to_string())
                }
                b"og:image" | b"og:image:secure_url" | b"twitter:image" | b"twitter:image:src" => {
                    if metadata.image.is_some() {
                        continue;
                    }
                    let image = metadata.image.get_or_insert_with(Default::default);
                    image.url = value.as_utf8_str().to_string();
                }
                b"og:image:width" => {
                    let image = metadata.image.get_or_insert_with(Default::default);
                    image.width = value.as_utf8_str().parse().unwrap_or_default();
                }
                b"og:image:height" => {
                    let image = metadata.image.get_or_insert_with(Default::default);
                    image.height = value.as_utf8_str().parse().unwrap_or_default();
                }
                b"og:video" | b"og:video:secure_url" | b"twitter:video" | b"twitter:video:src" => {
                    if metadata.video.is_some() {
                        continue;
                    }
                    let video = metadata.video.get_or_insert_with(Default::default);
                    video.url = value.as_utf8_str().to_string();
                }
                b"og:video:width" => {
                    let video = metadata.video.get_or_insert_with(Default::default);
                    video.width = value.as_utf8_str().parse().unwrap_or_default();
                }
                b"og:video:height" => {
                    let video = metadata.video.get_or_insert_with(Default::default);
                    video.height = value.as_utf8_str().parse().unwrap_or_default();
                }
                b"twitter:card" => {
                    if value.eq("summary_large_image") {
                        let image = metadata.image.get_or_insert_with(Default::default);
                        image.size = ImageSize::Large
                    }
                }
                b"theme-color" => metadata.colour = Some(value.as_utf8_str().to_string()),
                b"og:type" => metadata.opengraph_type = Some(value.as_utf8_str().to_string()),
                b"og:site_name" => metadata.site_name = Some(value.as_utf8_str().to_string()),
                b"og:url" => metadata.url = value.as_utf8_str().to_string(),
                _ => (),
            }
        }

        Ok(metadata)
    }

    async fn resolve_image(&mut self) -> Result<(), Error> {
        if let Some(image) = &mut self.image {
            // If image WxH was already provided by OpenGraph,
            // just return that instead.
            if image.width != 0 && image.height != 0 {
                return Ok(());
            }

            let (resp, _) = fetch(&image.url).await?;
            let (width, height) = consume_size(resp).await?;

            image.width = width;
            image.height = height;
        }

        Ok(())
    }

    pub async fn generate_special(&mut self) -> Result<Special, Error> {
        lazy_static! {
            // ! FIXME: use youtube-dl to fetch metadata
            static ref RE_YOUTUBE: Regex = Regex::new("^(?:(?:https?:)?//)?(?:(?:www|m)\\.)?(?:(?:youtube\\.com|youtu.be))(?:/(?:[\\w\\-]+\\?v=|embed/|v/)?)([\\w\\-]+)(?:\\S+)?$").unwrap();

            // ! FIXME: use Twitch API to fetch metadata
            static ref RE_TWITCH: Regex = Regex::new("^(?:https?://)?(?:www\\.|go\\.)?twitch\\.tv/([a-z0-9_]+)($|\\?)").unwrap();
            static ref RE_TWITCH_VOD: Regex = Regex::new("^(?:https?://)?(?:www\\.|go\\.)?twitch\\.tv/videos/([0-9]+)($|\\?)").unwrap();
            static ref RE_TWITCH_CLIP: Regex = Regex::new("^(?:https?://)?(?:www\\.|go\\.)?twitch\\.tv/(?:[a-z0-9_]+)/clip/([A-z0-9_-]+)($|\\?)").unwrap();

            static ref RE_SPOTIFY: Regex = Regex::new("^(?:https?://)?open.spotify.com/(track|user|artist|album|playlist)/([A-z0-9]+)").unwrap();
            static ref RE_SOUNDCLOUD: Regex = Regex::new("^(?:https?://)?soundcloud.com/([a-zA-Z0-9-]+)/([A-z0-9-]+)").unwrap();
            static ref RE_BANDCAMP: Regex = Regex::new("^(?:https?://)?(?:[A-z0-9_-]+).bandcamp.com/(track|album)/([A-z0-9_-]+)").unwrap();
        }

        if let Some(captures) = RE_YOUTUBE.captures_iter(&self.url).next() {
            lazy_static! {
                static ref RE_TIMESTAMP: Regex =
                    Regex::new("(?:\\?|&)(?:t|start)=([\\w]+)").unwrap();
            }

            if let Some(video) = &self.video {
                if let Some(timestamp_captures) = RE_TIMESTAMP.captures_iter(&video.url).next() {
                    return Ok(Special::YouTube {
                        id: captures[1].to_string(),
                        timestamp: Some(timestamp_captures[1].to_string()),
                    });
                }

                return Ok(Special::YouTube {
                    id: captures[1].to_string(),
                    timestamp: None,
                });
            }
        } else if let Some(captures) = RE_TWITCH.captures_iter(&self.url).next() {
            return Ok(Special::Twitch {
                id: captures[1].to_string(),
                content_type: TwitchType::Channel,
            });
        } else if let Some(captures) = RE_TWITCH_VOD.captures_iter(&self.url).next() {
            return Ok(Special::Twitch {
                id: captures[1].to_string(),
                content_type: TwitchType::Video,
            });
        } else if let Some(captures) = RE_TWITCH_CLIP.captures_iter(&self.url).next() {
            return Ok(Special::Twitch {
                id: captures[1].to_string(),
                content_type: TwitchType::Clip,
            });
        } else if let Some(captures) = RE_SPOTIFY.captures_iter(&self.url).next() {
            return Ok(Special::Spotify {
                content_type: captures[1].to_string(),
                id: captures[2].to_string(),
            });
        } else if RE_SOUNDCLOUD.is_match(&self.url) {
            return Ok(Special::Soundcloud);
        } else if RE_BANDCAMP.is_match(&self.url) {
            lazy_static! {
                static ref RE_TRACK: Regex = Regex::new("track=(\\d+)").unwrap();
                static ref RE_ALBUM: Regex = Regex::new("album=(\\d+)").unwrap();
            }

            if let Some(video) = &self.video {
                if let Some(captures) = RE_TRACK.captures_iter(&video.url).next() {
                    return Ok(Special::Bandcamp {
                        content_type: BandcampType::Track,
                        id: captures[1].to_string(),
                    });
                }

                if let Some(captures) = RE_ALBUM.captures_iter(&video.url).next() {
                    return Ok(Special::Bandcamp {
                        content_type: BandcampType::Album,
                        id: captures[1].to_string(),
                    });
                }
            }
        }

        Ok(Special::None)
    }

    pub async fn resolve_external(&mut self) {
        if let Ok(special) = self.generate_special().await {
            self.special = Some(special);
        }

        if self.resolve_image().await.is_err() {
            self.image = None;
        }
    }

    pub fn is_none(&self) -> bool {
        self.title.is_none() && self.description.is_none() && self.image.is_none()
    }
}
