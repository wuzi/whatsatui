//! Public screenshot fixtures: fictional identities, English messages, bundled photos.
use super::*;

pub(super) struct Profiles;

#[async_trait::async_trait]
impl crate::avatars::Provider for Profiles {
    async fn fetch(&self, identity: &crate::avatars::Identity) -> Result<Option<Vec<u8>>, String> {
        if identity.account.0 != ACCOUNT {
            return Ok(None);
        }
        let bytes: &[u8] = match identity.jid.as_str() {
            "you@demo" => include_bytes!("../../../assets/demo/you.jpg"),
            "alice@demo" => include_bytes!("../../../assets/demo/alice.jpg"),
            "maya@demo" => include_bytes!("../../../assets/demo/maya.jpg"),
            "leo@demo" => include_bytes!("../../../assets/demo/leo.jpg"),
            "priya@demo" => include_bytes!("../../../assets/demo/priya.jpg"),
            "noah@demo" => include_bytes!("../../../assets/demo/noah.jpg"),
            "sofia@demo" => include_bytes!("../../../assets/demo/sofia.jpg"),
            "oliver@demo" => include_bytes!("../../../assets/demo/oliver.jpg"),
            "lena@demo" => include_bytes!("../../../assets/demo/lena.jpg"),
            "weekend@g.us" | "coffee@g.us" | "books@g.us" | "kitchen@g.us" => IMAGE,
            "trail@g.us" | "rides@g.us" | "neighbors@g.us" | "market@g.us" => {
                include_bytes!("../../../assets/demo/trail.jpg")
            }
            _ => return Ok(None),
        };
        Ok(Some(bytes.to_vec()))
    }
}

pub(super) async fn initialize(store: &Store) -> Result<StoreChange, BackendError> {
    // A stable timeline keeps screenshots reproducible. Only the interactive
    // demo's newly sent messages use the current clock.
    let chats = [
        ("weekend@g.us", "Weekend plans ☕", 4, "maya@demo", "", 47),
        ("alice@demo", "Alice Morgan", 0, "alice@demo", "", 25),
        (
            "design@g.us",
            "Design lab",
            12,
            "sofia@demo",
            "The new color palette looks great!",
            24,
        ),
        (
            "maya@demo",
            "Maya Brooks",
            2,
            "maya@demo",
            "Sending you the playlist in a minute 🎶",
            23,
        ),
        (
            "trail@g.us",
            "Trail crew 🥾",
            6,
            "noah@demo",
            "Clear skies for the morning hike",
            22,
        ),
        ("leo@demo", "Leo Turner", 0, "leo@demo", "", 21),
        (
            "priya@demo",
            "Priya Shah",
            1,
            "priya@demo",
            "Got the photos — thank you!",
            20,
        ),
        (
            "coffee@g.us",
            "Coffee club",
            8,
            "maya@demo",
            "One more vote for the corner café",
            19,
        ),
        (
            "noah@demo",
            "Noah Reed",
            0,
            "noah@demo",
            "I'll bring the board games",
            18,
        ),
        (
            "games@g.us",
            "Friday games 🎲",
            23,
            "leo@demo",
            "Same time next week?",
            17,
        ),
        (
            "sofia@demo",
            "Sofia Chen",
            0,
            "sofia@demo",
            "This is exactly what I had in mind",
            16,
        ),
        (
            "lena@demo",
            "Lena Carter",
            3,
            "lena@demo",
            "Saved you a slice 🍰",
            15,
        ),
        (
            "books@g.us",
            "Book club 📚",
            5,
            "oliver@demo",
            "No spoilers until Sunday, please!",
            14,
        ),
        (
            "oliver@demo",
            "Oliver Grant",
            0,
            "oliver@demo",
            "See you at the workshop",
            13,
        ),
        (
            "projects@g.us",
            "Side projects",
            9,
            "priya@demo",
            "Just shipped the first version 🚀",
            12,
        ),
        (
            "films@g.us",
            "Film night 🍿",
            4,
            "sofia@demo",
            "I'll handle the popcorn",
            11,
        ),
        (
            "market@g.us",
            "Sunday market",
            0,
            "lena@demo",
            "The flower stall is back this week",
            10,
        ),
        (
            "kitchen@g.us",
            "Kitchen experiments",
            7,
            "noah@demo",
            "Update: the bread actually rose!",
            9,
        ),
        (
            "neighbors@g.us",
            "Neighborhood",
            0,
            "alice@demo",
            "Thanks for watering the plants 🌱",
            8,
        ),
        (
            "rides@g.us",
            "Weekend rides",
            2,
            "leo@demo",
            "Taking the scenic route this time",
            7,
        ),
        (
            "makers@g.us",
            "Makerspace",
            0,
            "oliver@demo",
            "The little lamp is finally working",
            6,
        ),
        (
            "study@g.us",
            "Study room",
            0,
            "priya@demo",
            "Notes are ready for tomorrow",
            5,
        ),
    ];
    store
        .upsert_chats(
            ACCOUNT.into(),
            chats
                .iter()
                .map(|&(id, name, unread, _, _, minute)| ChatSummary {
                    account: ACCOUNT.into(),
                    chat: id.into(),
                    name: name.into(),
                    is_group: id.ends_with("@g.us"),
                    unread,
                    latest_at_ms: TIME + minute * 60_000,
                    ..Default::default()
                })
                .collect(),
        )
        .await
        .map_err(|e| BackendError::Service(e.into()))?;

    let mut messages: Vec<_> = chats
        .iter()
        .filter(|(_, _, _, _, preview, _)| !preview.is_empty())
        .map(|&(chat, _, _, sender, preview, minute)| {
            message(chat, sender, "preview", preview, minute * 60_000)
        })
        .collect();
    messages.extend([
        message("alice@demo", "alice@demo", "a1", "Hey! How is the new terminal setup going?", 23 * 60_000),
        message("alice@demo", ACCOUNT, "a2", "Cyan borders, a good keyboard, and no lost drafts.", 24 * 60_000),
        message("alice@demo", "alice@demo", "a3", "Tab moves between panes. Ctrl-P finds chats; Ctrl-F searches this conversation. Try cyan.\n*Message actions*: Enter in Messages, y to copy, o for links.\n_Italic_ ~old~ `code`\nhttps://example.org/whatsapp-tui", 25 * 60_000),
        message("weekend@g.us", "maya@demo", "g1", "Saturday coffee, then a walk by the river? ☕", 40 * 60_000),
        message("weekend@g.us", "leo@demo", "g2", "I'm in. I'll bring the book I promised you.", 41 * 60_000),
        message("weekend@g.us", ACCOUNT, "g4", "This place looks perfect.", 43 * 60_000),
        message("weekend@g.us", ACCOUNT, "g5", "10:30 works for me — save me a croissant 😄", 43 * 60_000 + 30_000),
        message("weekend@g.us", "priya@demo", "g6", "Count me in! I'll bring my camera.", 44 * 60_000),
        message("weekend@g.us", "leo@demo", "g7", "A coffee walk somehow became a photo walk 😂", 45 * 60_000),
        message("weekend@g.us", "maya@demo", "g8", "Deal. Table outside if the weather holds!", 46 * 60_000),
        message("weekend@g.us", ACCOUNT, "g9", "See you all Saturday ☀️", 47 * 60_000),
    ]);
    let mut reply = message(
        "weekend@g.us",
        "lena@demo",
        "g-reply",
        "The croissants alone sold me on this plan.",
        46 * 60_000 + 30_000,
    );
    reply.quote = Some(Quote {
        key: key("weekend@g.us", ACCOUNT, "g5"),
        preview: "10:30 works for me — save me a croissant 😄".into(),
        media_kind: None,
        availability: QuoteAvailability::Available,
    });
    messages.push(reply);
    for (chat, sender, id, attachment, offset) in [
        (
            "weekend@g.us",
            "maya@demo",
            "g3",
            image_attachment(),
            42 * 60_000,
        ),
        (
            "leo@demo",
            "leo@demo",
            "l-video",
            video_attachment(),
            19 * 60_000,
        ),
        (
            "leo@demo",
            "leo@demo",
            "l-sticker",
            sticker_attachment(),
            21 * 60_000,
        ),
        (
            "maya@demo",
            "maya@demo",
            "m-audio",
            audio_attachment(),
            22 * 60_000,
        ),
    ] {
        let mut media = message(chat, sender, id, "", offset);
        media.body = MessageBody::Media(Box::new(attachment));
        messages.push(media);
    }
    let mut changes: Vec<_> = messages.into_iter().map(MessageChange::Upsert).collect();
    for (chat, sender, id, reactor, emoji, minute) in [
        ("alice@demo", "alice@demo", "a3", ACCOUNT, "👍", 26),
        ("alice@demo", "alice@demo", "a3", "alice@demo", "👍", 26),
        ("weekend@g.us", "maya@demo", "g3", ACCOUNT, "❤️", 43),
        ("weekend@g.us", "maya@demo", "g3", "priya@demo", "❤️", 44),
        ("weekend@g.us", "leo@demo", "g7", "maya@demo", "😂", 46),
    ] {
        changes.push(MessageChange::Reaction(Reaction {
            key: key(chat, sender, id),
            reactor: reactor.into(),
            emoji: emoji.into(),
            at_ms: TIME + minute * 60_000,
            event_id: format!("demo-reaction-{id}-{reactor}").into(),
        }));
    }
    store
        .apply_batch(MessageBatch {
            account: ACCOUNT.into(),
            source: MessageSource::History,
            changes,
        })
        .await
        .map_err(|e| BackendError::Service(e.into()))
}
