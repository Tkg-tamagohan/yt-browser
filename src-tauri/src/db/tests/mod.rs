mod ddl;
mod feed;
mod filters;
mod history;
mod library;
mod migrations;
mod settings;

fn vref(video_id: &str, title: &str) -> crate::model::VideoRef {
    crate::model::VideoRef {
        video_id: video_id.to_string(),
        title: title.to_string(),
        channel_id: Some("UCchan000000000000001".to_string()),
        channel_title: Some("テストCH".to_string()),
        thumbnail_url: Some("https://i.ytimg.com/vi/x.jpg".to_string()),
    }
}
