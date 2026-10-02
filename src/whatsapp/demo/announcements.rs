//! Fictional business fixtures passed through the real native normalizer.
use super::*;
use whatsapp_rust::prelude::{MessageField as F, wa};

pub(super) fn messages() -> Vec<MessageChange> {
    use wa::message::{interactive_message as im, template_message as tm};
    let source = image_attachment();
    let image = wa::message::ImageMessage {
        mimetype: source.mime,
        file_length: Some(source.size),
        direct_path: Some(source.direct_path),
        media_key: Some(source.media_key.to_vec()),
        file_sha256: Some(source.sha256.to_vec()),
        file_enc_sha256: Some(source.encrypted_sha256.to_vec()),
        ..Default::default()
    };
    let template = wa::Message {
        template_message: F::some(wa::message::TemplateMessage {
            hydrated_template: F::some(tm::HydratedFourRowTemplate {
                title: Some(tm::hydrated_four_row_template::Title::ImageMessage(
                    Box::new(image.clone()),
                )),
                hydrated_content_text: Some(
                    "Weekend offers ☕\nFresh coffee and croissants are back!".into(),
                ),
                hydrated_footer_text: Some("Fictional store · demo announcement".into()),
                hydrated_buttons: vec![wa::HydratedTemplateButton {
                    hydrated_button: Some(wa::hydrated_template_button::HydratedButton::UrlButton(
                        Box::new(wa::hydrated_template_button::HydratedURLButton {
                            display_text: Some("View offers".into()),
                            url: Some("https://example.org/offers".into()),
                            ..Default::default()
                        }),
                    )),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    let interactive = wa::Message {
        interactive_message: F::some(wa::message::InteractiveMessage {
            header: F::some(im::Header {
                title: Some("Your order is ready".into()),
                media: Some(im::header::Media::ImageMessage(Box::new(image))),
                ..Default::default()
            }),
            body: F::some(im::Body { text: Some("Pick up your coffee order whenever you are ready.".into()) }),
            footer: F::some(im::Footer { text: Some("Open today until 18:00".into()), ..Default::default() }),
            interactive_message: Some(im::InteractiveMessage::NativeFlowMessage(Box::new(im::NativeFlowMessage {
                buttons: [
                    ("cta_url", serde_json::json!({"display_text":"View order", "url":"https://example.org/order/42"})),
                    ("quick_reply", serde_json::json!({"display_text":"Thank you!", "id":"synthetic-callback"})),
                ].into_iter().map(|(name, params)| im::native_flow_message::NativeFlowButton { name: Some(name.into()), button_params_json: Some(params.to_string()) }).collect(),
                ..Default::default()
            }))),
            ..Default::default()
        }),
        ..Default::default()
    };
    [template, interactive]
        .into_iter()
        .enumerate()
        .map(|(index, payload)| {
            super::super::normalize::normalize(
                key("aster@demo", "aster@demo", &format!("announcement-{index}")),
                &payload,
                TIME + (3 + index as i64) * 60_000,
                None,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::Downloader;

    #[tokio::test]
    async fn business_headers_download_only_the_exact_offline_fixture() {
        let root = tempfile::tempdir().unwrap();
        let (_stop, cancel) = tokio::sync::watch::channel(false);
        for (index, change) in messages().into_iter().enumerate() {
            let MessageChange::Upsert(message) = change else {
                panic!()
            };
            let MessageBody::Media(mut attachment) = message.body else {
                panic!()
            };
            let path = root.path().join(format!("image-{index}.jpg"));
            DemoDownloader
                .download(&attachment, &path, cancel.clone())
                .await
                .unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), IMAGE);
            attachment.media_key[0] ^= 1;
            let rejected = root.path().join(format!("rejected-{index}.jpg"));
            assert!(
                DemoDownloader
                    .download(&attachment, &rejected, cancel.clone())
                    .await
                    .is_err()
            );
            assert!(!rejected.exists());
        }
    }
}
