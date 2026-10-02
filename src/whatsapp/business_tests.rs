use super::normalize;
use crate::{app::model::*, message_actions};
use wa::message::{interactive_message as im, template_message as tm};
use whatsapp_rust::prelude::{MessageBuilderExt, MessageField as F, wa};

fn key() -> MessageKey {
    MessageKey {
        account: "self@s.whatsapp.net".into(),
        chat: "shop@s.whatsapp.net".into(),
        sender: "shop@s.whatsapp.net".into(),
        id: "announcement".into(),
        from_me: false,
    }
}
fn record(payload: &wa::Message) -> MessageRecord {
    let MessageChange::Upsert(message) = normalize::normalize(key(), payload, 1_000, None) else {
        panic!("expected announcement")
    };
    message
}
fn text(message: &MessageRecord) -> &str {
    message_actions::text(message, 1_000).expect("announcement must be readable")
}
fn image() -> wa::message::ImageMessage {
    wa::message::ImageMessage {
        mimetype: Some("image/jpeg".into()),
        file_length: Some(32),
        direct_path: Some("/o1/v/business-image".into()),
        media_key: Some(vec![1; 32]),
        file_sha256: Some(vec![2; 32]),
        file_enc_sha256: Some(vec![3; 32]),
        ..Default::default()
    }
}
fn hydrated() -> tm::HydratedFourRowTemplate {
    use wa::hydrated_template_button::{
        HydratedButton as B, HydratedQuickReplyButton, HydratedURLButton,
    };
    tm::HydratedFourRowTemplate {
        title: Some(tm::hydrated_four_row_template::Title::HydratedTitleText(
            "Weekend offers".into(),
        )),
        hydrated_content_text: Some("Fresh coffee is back ☕".into()),
        hydrated_footer_text: Some("Offer ends Sunday".into()),
        hydrated_buttons: vec![
            wa::HydratedTemplateButton {
                hydrated_button: Some(B::UrlButton(Box::new(HydratedURLButton {
                    display_text: Some("View offers".into()),
                    url: Some("https://example.org/offers?source=chat".into()),
                    ..Default::default()
                }))),
                ..Default::default()
            },
            wa::HydratedTemplateButton {
                hydrated_button: Some(B::QuickReplyButton(Box::new(HydratedQuickReplyButton {
                    display_text: Some("Stop updates".into()),
                    id: Some("opaque-server-callback".into()),
                }))),
                ..Default::default()
            },
        ],
        ..Default::default()
    }
}
fn template(value: tm::HydratedFourRowTemplate) -> wa::Message {
    wa::Message {
        template_message: F::some(wa::message::TemplateMessage {
            hydrated_template: F::some(value),
            ..Default::default()
        }),
        ..Default::default()
    }
}
fn interactive() -> wa::message::InteractiveMessage {
    wa::message::InteractiveMessage {
        header: F::some(im::Header {
            title: Some("Your delivery".into()),
            media: Some(im::header::Media::ImageMessage(Box::new(image()))),
            ..Default::default()
        }),
        body: F::some(im::Body { text: Some("The courier is on the way.".into()) }),
        footer: F::some(im::Footer { text: Some("Aster Market".into()), ..Default::default() }),
        interactive_message: Some(im::InteractiveMessage::NativeFlowMessage(Box::new(im::NativeFlowMessage {
            buttons: vec![
                im::native_flow_message::NativeFlowButton {
                    name: Some("cta_url".into()),
                    button_params_json: Some(r#"{"display_text":"Track delivery","url":"https://example.org/track","merchant_url":"https://hidden.example.org","token":"internal-token"}"#.into()),
                },
                im::native_flow_message::NativeFlowButton {
                    name: Some("quick_reply".into()),
                    button_params_json: Some(r#"{"display_text":"Thank you","id":"secret-callback"}"#.into()),
                },
            ],
            ..Default::default()
        }))),
        ..Default::default()
    }
}

#[test]
fn hydrated_template_variants_expose_text_and_links_without_callback_ids() {
    let mut first = template(hydrated());
    let base = first.template_message.as_option_mut().unwrap();
    base.context_info = F::some(wa::ContextInfo {
        expiration: Some(60),
        stanza_id: Some("original".into()),
        participant: Some("self@s.whatsapp.net".into()),
        quoted_message: F::some(wa::Message::text("Any offers?")),
        ..Default::default()
    });
    let mut alternate = first.clone();
    let value = alternate.template_message.as_option_mut().unwrap();
    value.hydrated_template = F::none();
    value.format = Some(tm::Format::HydratedFourRowTemplate(Box::new(hydrated())));
    let hsm = wa::Message {
        highly_structured_message: F::some(wa::message::HighlyStructuredMessage {
            hydrated_hsm: first.template_message.clone(),
            ..Default::default()
        }),
        ..Default::default()
    };
    for payload in [first, alternate, hsm] {
        let message = record(&payload);
        for expected in [
            "Weekend offers",
            "Fresh coffee is back ☕",
            "Offer ends Sunday",
            "View offers",
            "Stop updates",
        ] {
            assert!(text(&message).contains(expected), "missing {expected}");
        }
        assert!(!text(&message).contains("opaque-server-callback"));
        assert_eq!(
            message_actions::web_links(text(&message)),
            ["https://example.org/offers?source=chat"]
        );
        assert_eq!(message.expires_at_ms, Some(61_000));
        assert_eq!(message.quote.as_ref().unwrap().preview, "Any offers?");
    }
}

#[test]
fn interactive_image_keeps_download_keys_and_button_links_in_live_and_history() {
    let payload = wa::Message {
        interactive_message: F::some(interactive()),
        ..Default::default()
    };
    let live = record(&payload);
    let web = wa::WebMessageInfo {
        key: F::some(wa::MessageKey {
            id: Some("announcement".into()),
            ..Default::default()
        }),
        message: F::some(payload),
        message_timestamp: Some(1),
        ..Default::default()
    };
    let MessageChange::Upsert(history) =
        normalize::history_message(&key().account, &key().chat, &web).unwrap()
    else {
        panic!()
    };
    assert_eq!(live.body, history.body);
    let MessageBody::Media(attachment) = &live.body else {
        panic!("business image lost its preview")
    };
    assert_eq!(attachment.kind, crate::media::AttachmentKind::Image);
    assert_eq!(attachment.media_key, [1; 32]);
    assert_eq!(attachment.direct_path, "/o1/v/business-image");
    for expected in [
        "Your delivery",
        "The courier is on the way.",
        "Aster Market",
        "Track delivery",
        "Thank you",
    ] {
        assert!(text(&live).contains(expected));
    }
    for private in ["internal-token", "hidden.example.org", "secret-callback"] {
        assert!(!text(&live).contains(private));
    }
    assert_eq!(
        message_actions::web_links(text(&live)),
        ["https://example.org/track"]
    );
}

#[test]
fn business_text_survives_missing_or_view_once_image_references() {
    for mode in 0..3 {
        let mut value = hydrated();
        let mut photo = image();
        if mode == 0 {
            photo.media_key = None;
        }
        if mode == 1 {
            photo.view_once = Some(true);
        }
        value.title = Some(tm::hydrated_four_row_template::Title::ImageMessage(
            Box::new(photo),
        ));
        let mut payload = template(value);
        if mode == 2 {
            payload = wa::Message {
                view_once_message_v2: F::some(wa::message::FutureProofMessage {
                    message: F::some(payload),
                }),
                ..Default::default()
            };
        }
        let message = record(&payload);
        assert!(text(&message).contains("Fresh coffee is back ☕"));
        assert!(!matches!(message.body, MessageBody::Media(_)));
    }
}

#[test]
fn legacy_buttons_and_lists_keep_visible_choices_without_ids() {
    let buttons = wa::Message {
        buttons_message: F::some(wa::message::ButtonsMessage {
            content_text: Some("Your appointment is tomorrow".into()),
            footer_text: Some("Arrive ten minutes early".into()),
            buttons: vec![wa::message::buttons_message::Button {
                button_id: Some("callback-secret".into()),
                button_text: F::some(wa::message::buttons_message::button::ButtonText {
                    display_text: Some("Confirm appointment".into()),
                }),
                ..Default::default()
            }],
            ..Default::default()
        }),
        ..Default::default()
    };
    let message = record(&buttons);
    for expected in [
        "Your appointment is tomorrow",
        "Arrive ten minutes early",
        "Confirm appointment",
    ] {
        assert!(text(&message).contains(expected));
    }
    assert!(!text(&message).contains("callback-secret"));
    let list = wa::Message {
        list_message: F::some(wa::message::ListMessage {
            title: Some("Choose a pickup slot".into()),
            description: Some("Collection is free".into()),
            button_text: Some("View slots".into()),
            sections: vec![wa::message::list_message::Section {
                title: Some("Tomorrow".into()),
                rows: vec![wa::message::list_message::Row {
                    title: Some("Morning".into()),
                    description: Some("9am to noon".into()),
                    row_id: Some("hidden-row-id".into()),
                }],
            }],
            ..Default::default()
        }),
        ..Default::default()
    };
    let message = record(&list);
    for expected in [
        "Choose a pickup slot",
        "Collection is free",
        "View slots",
        "Tomorrow",
        "Morning",
        "9am to noon",
    ] {
        assert!(text(&message).contains(expected));
    }
    assert!(!text(&message).contains("hidden-row-id"));
}

#[test]
fn replies_to_business_templates_have_a_readable_original_preview() {
    let payload = wa::Message {
        extended_text_message: F::some(wa::message::ExtendedTextMessage {
            text: Some("Is this still available?".into()),
            context_info: F::some(wa::ContextInfo {
                stanza_id: Some("offer".into()),
                participant: Some("shop@s.whatsapp.net".into()),
                quoted_message: F::some(template(hydrated())),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    let quote = record(&payload).quote.unwrap();
    assert_eq!(quote.availability, QuoteAvailability::Available);
    assert!(quote.preview.contains("Fresh coffee is back ☕"));
    assert_eq!(quote.key.id.0, "offer");
}

#[test]
fn image_headers_work_in_legacy_and_interactive_template_containers() {
    let mut value = hydrated();
    value.title = Some(tm::hydrated_four_row_template::Title::ImageMessage(
        Box::new(image()),
    ));
    let templates = [
        template(value),
        wa::Message {
            buttons_message: F::some(wa::message::ButtonsMessage {
                header: Some(wa::message::buttons_message::Header::ImageMessage(
                    Box::new(image()),
                )),
                content_text: Some("Your delivery".into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        wa::Message {
            template_message: F::some(wa::message::TemplateMessage {
                format: Some(tm::Format::InteractiveMessageTemplate(Box::new(
                    interactive(),
                ))),
                ..Default::default()
            }),
            ..Default::default()
        },
    ];
    for payload in templates {
        let message = record(&payload);
        let MessageBody::Media(attachment) = &message.body else {
            panic!("missing image header")
        };
        assert_eq!(attachment.kind, crate::media::AttachmentKind::Image);
        assert!(!text(&message).is_empty());
        assert!(
            message_actions::available(&message, 1_000)
                .contains(&crate::config::bindings::ActionId::DownloadMedia)
        );
    }
}

#[test]
fn native_button_fields_are_bounded_and_unknown_payloads_never_replace_the_body() {
    for json in [
        "{broken".into(),
        "[]".into(),
        r#"{"display_text":42,"url":[]}"#.into(),
        "x".repeat(70_000),
        r#"{"display_text":"Open","url":"javascript:alert(1)","token":"never-display"}"#.into(),
    ] {
        let mut value = interactive();
        let Some(im::InteractiveMessage::NativeFlowMessage(flow)) = &mut value.interactive_message
        else {
            panic!()
        };
        flow.buttons[0].button_params_json = Some(json);
        let message = record(&wa::Message {
            interactive_message: F::some(value),
            ..Default::default()
        });
        assert!(text(&message).contains("The courier is on the way."));
        assert!(text(&message).contains("Thank you"));
        assert!(!text(&message).contains("javascript:"));
        assert!(!text(&message).contains("never-display"));
        assert!(message_actions::web_links(text(&message)).is_empty());
    }
    let mut value = interactive();
    value.body = F::some(im::Body {
        text: Some("☕".repeat(40_000)),
    });
    let message = record(&wa::Message {
        interactive_message: F::some(value),
        ..Default::default()
    });
    assert!(text(&message).len() < 33_000);
    assert!(text(&message).contains("More content available"));
    // A rejected, oversized part cannot leave an actionable half-URL behind.
    assert!(message_actions::web_links(text(&message)).is_empty());
}

#[test]
fn native_lists_copy_codes_and_phone_labels_use_only_visible_fields() {
    let mut value = interactive();
    let Some(im::InteractiveMessage::NativeFlowMessage(flow)) = &mut value.interactive_message
    else {
        panic!()
    };
    flow.buttons = [
        ("single_select", r#"{"title":"Pickup slots","sections":[{"title":"Tomorrow","rows":[{"title":"Morning","description":"9am to noon","id":"row-secret"}]}]}"#),
        ("cta_copy", r#"{"display_text":"Discount code","copy_code":"COFFEE10","id":"copy-secret"}"#),
        ("cta_call", r#"{"display_text":"Call support","phone_number":"+1 555 0100"}"#),
        ("unknown_flow", r#"{"display_text":"More options","flow_token":"hidden-token"}"#),
    ].into_iter().map(|(name, json)| im::native_flow_message::NativeFlowButton { name: Some(name.into()), button_params_json: Some(json.into()) }).collect();
    let message = record(&wa::Message {
        interactive_message: F::some(value),
        ..Default::default()
    });
    for expected in [
        "Pickup slots",
        "Tomorrow",
        "Morning",
        "9am to noon",
        "Discount code",
        "COFFEE10",
        "Call support",
        "+1 555 0100",
        "More options",
    ] {
        assert!(text(&message).contains(expected));
    }
    for hidden in ["row-secret", "copy-secret", "hidden-token"] {
        assert!(!text(&message).contains(hidden));
    }
}

#[tokio::test]
async fn history_replay_upgrades_a_placeholder_and_persists_searchable_announcement_text() {
    use crate::storage::Store;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("business.sqlite3");
    let store = Store::open(path.clone()).await.unwrap();
    let mut old = record(&template(hydrated()));
    old.body = MessageBody::Unsupported {
        kind: "unsupported message".into(),
        caption: None,
    };
    let batch = |message| MessageBatch {
        account: key().account,
        source: MessageSource::History,
        changes: vec![MessageChange::Upsert(message)],
    };
    store.apply_batch(batch(old)).await.unwrap();
    let new = record(&template(hydrated()));
    store.apply_batch(batch(new.clone())).await.unwrap();
    store.apply_batch(batch(new.clone())).await.unwrap();
    store.flush().await.unwrap();
    let reopened = Store::open(path).await.unwrap();
    let restored = reopened.get_message(key()).await.unwrap().unwrap();
    assert_eq!(restored.body, new.body);
    assert!(
        message_actions::available(&restored, 1_000)
            .contains(&crate::config::bindings::ActionId::OpenLinks)
    );
    let found = reopened
        .search_messages(key().account, key().chat, "coffee".into(), 1_000)
        .await
        .unwrap();
    assert_eq!(found.hits.len(), 1);
    assert_eq!(found.hits[0].key, key());
    reopened
        .apply_batch(MessageBatch {
            account: key().account,
            source: MessageSource::Live,
            changes: vec![MessageChange::Delete { key: key() }],
        })
        .await
        .unwrap();
    reopened.apply_batch(batch(new)).await.unwrap();
    assert_eq!(
        reopened.get_message(key()).await.unwrap().unwrap().body,
        MessageBody::Deleted
    );
}
