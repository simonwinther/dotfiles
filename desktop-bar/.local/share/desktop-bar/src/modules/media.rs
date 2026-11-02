//! MPRIS discovery and playback updates never run on the rendering thread.
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};
use zbus::{blocking::Proxy, zvariant::OwnedValue};

const PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";
type Properties = HashMap<String, OwnedValue>;

#[derive(Clone, Debug, Default)]
pub struct Media {
    pub bus: String,
    pub title: String,
    pub artist: String,
    pub identity: String,
    pub playing: bool,
    pub can_toggle: bool,
    pub can_next: bool,
    pub can_previous: bool,
    pub art_url: String,
    pub art: Option<Arc<tiny_skia::Pixmap>>,
}
impl Media {
    pub fn available(&self) -> bool {
        !self.bus.is_empty()
    }
}
fn text(values: &Properties, key: &str) -> String {
    values
        .get(key)
        .and_then(|v| <&str>::try_from(v).ok())
        .unwrap_or_default()
        .chars()
        .filter(|c| !c.is_control())
        .take(256)
        .collect()
}
fn flag(values: &Properties, key: &str) -> bool {
    values
        .get(key)
        .and_then(|v| bool::try_from(v).ok())
        .unwrap_or(false)
}
/// Reads a player. `identity` is its cached display name, which only changes
/// when the player restarts or reports a new Identity.
async fn read(
    connection: &zbus::Connection,
    name: &str,
    identity: Option<&str>,
) -> zbus::Result<Media> {
    let properties =
        zbus::Proxy::new(connection, name, PATH, "org.freedesktop.DBus.Properties").await?;
    let values: Properties = properties.call("GetAll", &(PLAYER,)).await?;
    let metadata = values
        .get("Metadata")
        .and_then(|v| v.try_clone().ok())
        .and_then(|v| Properties::try_from(v).ok())
        .unwrap_or_default();
    let artist = metadata
        .get("xesam:artist")
        .and_then(|v| v.try_clone().ok())
        .and_then(|v| Vec::<String>::try_from(v).ok())
        .unwrap_or_default()
        .join(", ");
    let identity = match identity {
        Some(identity) => identity.to_owned(),
        None => properties
            .call::<_, _, OwnedValue>("Get", &("org.mpris.MediaPlayer2", "Identity"))
            .await
            .ok()
            .and_then(|value| String::try_from(value).ok())
            .unwrap_or_else(|| name.trim_start_matches("org.mpris.MediaPlayer2.").into()),
    };
    let playing = text(&values, "PlaybackStatus") == "Playing";
    let control = flag(&values, "CanControl");
    Ok(Media {
        bus: name.into(),
        title: text(&metadata, "xesam:title"),
        artist,
        identity,
        playing,
        can_toggle: control && flag(&values, if playing { "CanPause" } else { "CanPlay" }),
        can_next: control && flag(&values, "CanGoNext"),
        can_previous: control && flag(&values, "CanGoPrevious"),
        art_url: metadata
            .get("mpris:artUrl")
            .and_then(|v| <&str>::try_from(v).ok())
            .filter(|url| url.len() <= 8192)
            .unwrap_or_default()
            .into(),
        art: None,
    })
}
fn visible_change(message: &zbus::Message) -> bool {
    if !message
        .header()
        .interface()
        .is_some_and(|i| i == "org.freedesktop.DBus.Properties")
        || !message
            .header()
            .member()
            .is_some_and(|m| m == "PropertiesChanged")
    {
        return false;
    }
    let Ok((interface, changed, invalidated)) = message
        .body()
        .deserialize::<(String, Properties, Vec<String>)>()
    else {
        return false;
    };
    let keys: &[&str] = match interface.as_str() {
        PLAYER => &[
            "Metadata",
            "PlaybackStatus",
            "CanControl",
            "CanPause",
            "CanPlay",
            "CanGoNext",
            "CanGoPrevious",
        ],
        "org.mpris.MediaPlayer2" => &["Identity"],
        _ => return false,
    };
    keys.iter()
        .any(|key| changed.contains_key(*key) || invalidated.iter().any(|value| value == key))
}

/// Whether two readings display identically; artwork is loaded separately.
fn same(a: &Media, b: &Media) -> bool {
    a.bus == b.bus
        && a.title == b.title
        && a.artist == b.artist
        && a.identity == b.identity
        && a.playing == b.playing
        && a.can_toggle == b.can_toggle
        && a.can_next == b.can_next
        && a.can_previous == b.can_previous
        && a.art_url == b.art_url
}

fn choose(players: &HashMap<String, (u64, Media)>) -> Media {
    players
        .values()
        .max_by_key(|(updated, media)| (media.playing, !media.title.is_empty(), *updated))
        .map(|(_, m)| m.clone())
        .unwrap_or_default()
}
pub fn action(bus: &str, method: &str) -> Result<(), String> {
    if !bus.starts_with("org.mpris.MediaPlayer2.")
        || !["PlayPause", "Next", "Previous"].contains(&method)
    {
        return Err("Unsupported playback action".into());
    }
    crate::bus::query(async {
        let connection = zbus::Connection::session().await?;
        let media = read(&connection, bus, None).await?;
        let allowed = match method {
            "PlayPause" => media.can_toggle,
            "Next" => media.can_next,
            _ => media.can_previous,
        };
        if !allowed {
            return Ok(());
        }
        zbus::Proxy::new(&connection, bus, PATH, PLAYER)
            .await?
            .call(method, &())
            .await
    })
    .map_err(|e| e.to_string())
}
pub fn listen(sender: smithay_client_toolkit::reexports::calloop::channel::Sender<super::Update>) {
    let (covers, jobs) = mpsc::channel::<(u64, String)>();
    let cover_generation = Arc::new(AtomicU64::new(0));
    let latest_cover = cover_generation.clone();
    let artwork = sender.clone();
    std::thread::Builder::new()
        .name("bar-artwork".into())
        .spawn(move || {
            let mut cache: HashMap<String, Arc<tiny_skia::Pixmap>> = HashMap::new();
            while let Ok((mut id, mut url)) = jobs.recv() {
                for (new_id, newer) in jobs.try_iter() {
                    id = new_id;
                    url = newer;
                }
                let image = cache.get(&url).cloned().or_else(|| {
                    if url.is_empty() {
                        return None;
                    }
                    let output = crate::process::output(
                        std::process::Command::new("desktopctl").args(["bar", "cover", &url]),
                        Duration::from_secs(8),
                        64 * 1024,
                        || id != latest_cover.load(Ordering::Relaxed),
                    )
                    .ok()?;
                    if !output.status.success() {
                        return None;
                    }
                    crate::assets::png(String::from_utf8_lossy(&output.stdout).trim()).map(Arc::new)
                });
                if let Some(image) = &image {
                    if cache.len() >= 4 {
                        cache.clear();
                    }
                    cache.insert(url.clone(), image.clone());
                }
                if id == latest_cover.load(Ordering::Relaxed)
                    && artwork.send(super::Update::MediaArt(url, image)).is_err()
                {
                    return;
                }
            }
        })
        .expect("artwork worker");
    std::thread::Builder::new()
        .name("bar-media".into())
        .spawn(move || {
            loop {
                let run = || -> zbus::Result<()> {
                    let connection = zbus::blocking::connection::Builder::session()?
                        .method_timeout(Duration::from_secs(2)).build()?;
                    let mut messages = crate::bus::Signals::new(&connection, &[
                        "type='signal',sender='org.freedesktop.DBus',interface='org.freedesktop.DBus',member='NameOwnerChanged',arg0namespace='org.mpris.MediaPlayer2'",
                        "type='signal',path='/org/mpris/MediaPlayer2',interface='org.freedesktop.DBus.Properties',member='PropertiesChanged',arg0namespace='org.mpris.MediaPlayer2'",
                    ])?;
                    let dbus = Proxy::new(
                        &connection,
                        "org.freedesktop.DBus",
                        "/org/freedesktop/DBus",
                        "org.freedesktop.DBus",
                    )?;
                    let mut players = HashMap::new();
                    let mut owners: HashMap<String, String> = HashMap::new();
                    let mut identities: HashMap<String, String> = HashMap::new();
                    let mut published: Option<Media> = None;
                    let mut revision = 0;
                    let mut art_url = String::new();
                    let names: Vec<String> = dbus.call("ListNames", &())?;
                    for name in names.into_iter().filter(|n| {
                        n.starts_with("org.mpris.MediaPlayer2.")
                            && n != "org.mpris.MediaPlayer2.playerctld"
                    }) {
                        if let Ok(media) = crate::bus::query(read(connection.inner(), &name, None)) {
                            identities.insert(name.clone(), media.identity.clone());
                            if let Ok(owner) = dbus.call::<_, _, String>("GetNameOwner", &(&name,))
                            {
                                owners.insert(owner, name.clone());
                            }
                            players.insert(name, (revision, media));
                            revision += 1;
                        }
                    }
                    let mut publish = |players: &HashMap<String, (u64, Media)>| {
                        let media = choose(players);
                        if art_url != media.art_url {
                            art_url = media.art_url.clone();
                            let id = cover_generation.fetch_add(1, Ordering::Relaxed) + 1;
                            let _ = covers.send((id, art_url.clone()));
                        }
                        // Players repeat unchanged metadata; only changes need a frame.
                        if published.as_ref().is_some_and(|last| same(last, &media)) {
                            return;
                        }
                        published = Some(media.clone());
                        let _ = sender.send(super::Update::Media(media));
                    };
                    publish(&players);
                    for message in &mut messages {
                        let message = message?;
                        let header = message.header();
                        let mut changed = None;
                        if header.member().is_some_and(|m| m == "NameOwnerChanged")
                            && header
                                .interface()
                                .is_some_and(|i| i == "org.freedesktop.DBus")
                        {
                            let (name, old, new): (String, String, String) =
                                message.body().deserialize()?;
                            if !name.starts_with("org.mpris.MediaPlayer2.")
                                || name == "org.mpris.MediaPlayer2.playerctld"
                            {
                                continue;
                            }
                            owners.remove(&old);
                            players.remove(&name);
                            identities.remove(&name);
                            if !new.is_empty() {
                                owners.insert(new, name.clone());
                                changed = Some(name);
                            }
                        } else if header.path().is_some_and(|p| p == PATH)
                            && visible_change(&message)
                        {
                            changed = header
                                .sender()
                                .and_then(|s| owners.get(s.as_str()))
                                .cloned();
                            let Some(name) = &changed else {
                                continue;
                            };
                            if message
                                .body()
                                .deserialize::<(String, Properties, Vec<String>)>()
                                .is_ok_and(|(interface, _, _)| interface == "org.mpris.MediaPlayer2")
                            {
                                identities.remove(name);
                            }
                        } else {
                            continue;
                        }
                        if let Some(name) = changed {
                            revision += 1;
                            let identity = identities.get(&name).map(String::as_str);
                            match crate::bus::query(read(connection.inner(), &name, identity)) {
                                Ok(media) => {
                                    identities.insert(name.clone(), media.identity.clone());
                                    players.insert(name, (revision, media));
                                }
                                Err(_) => {
                                    players.remove(&name);
                                    identities.remove(&name);
                                }
                            }
                        }
                        publish(&players);
                    }
                    Ok(())
                };
                let _ = run();
                let id = cover_generation.fetch_add(1, Ordering::Relaxed) + 1;
                let _ = covers.send((id, String::new()));
                let _ = sender.send(super::Update::Media(Media::default()));
                std::thread::sleep(Duration::from_secs(5));
            }
        })
        .expect("media worker");
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Root;
    #[zbus::interface(name = "org.mpris.MediaPlayer2")]
    impl Root {
        #[zbus(property)]
        fn identity(&self) -> String {
            "Fixture Player".into()
        }
    }
    struct Player;
    #[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
    impl Player {
        #[zbus(property)]
        fn playback_status(&self) -> String {
            "Playing".into()
        }
        #[zbus(property)]
        fn can_control(&self) -> bool {
            true
        }
        #[zbus(property)]
        fn can_pause(&self) -> bool {
            true
        }
        #[zbus(property)]
        fn metadata(&self) -> HashMap<String, OwnedValue> {
            HashMap::from([
                (
                    "xesam:title".into(),
                    OwnedValue::try_from(zbus::zvariant::Value::from("Song")).unwrap(),
                ),
                (
                    "xesam:artist".into(),
                    OwnedValue::try_from(zbus::zvariant::Value::from(vec!["Artist"])).unwrap(),
                ),
            ])
        }
    }
    #[test]
    fn players_are_read_once_with_identity_and_then_with_the_cached_name() {
        const TEST: &str = "modules::media::tests::players_are_read_once_with_identity_and_then_with_the_cached_name";
        if std::env::var_os("DESKTOP_BAR_PRIVATE_BUS_TEST").is_none() {
            let directory = std::env::temp_dir().join(format!(
                "desktop-bar-media-{}-{}",
                std::process::id(),
                super::super::clock::now()
            ));
            std::fs::create_dir(&directory).unwrap();
            let config = directory.join("session.conf");
            // Never include host bus policy, service activation, or listeners.
            std::fs::write(
                &config,
                r#"<busconfig>
                <type>session</type><listen>unix:tmpdir=/tmp</listen><auth>EXTERNAL</auth>
                <policy context="default"><allow send_destination="*"/>
                <allow receive_sender="*"/><allow own="*"/></policy>
                </busconfig>"#,
            )
            .unwrap();
            let status = std::process::Command::new("dbus-run-session")
                .arg("--config-file")
                .arg(&config)
                .arg("--")
                .arg(std::env::current_exe().unwrap())
                .args(["--exact", TEST, "--nocapture"])
                .env("DESKTOP_BAR_PRIVATE_BUS_TEST", "1")
                .status()
                .expect("dbus-run-session is required for media tests");
            std::fs::remove_file(config).unwrap();
            std::fs::remove_dir(directory).unwrap();
            assert!(status.success());
            return;
        }
        let name = "org.mpris.MediaPlayer2.fixture";
        let _player = zbus::blocking::connection::Builder::session()
            .unwrap()
            .name(name)
            .unwrap()
            .serve_at(PATH, Root)
            .unwrap()
            .serve_at(PATH, Player)
            .unwrap()
            .build()
            .unwrap();
        let client = zbus::blocking::Connection::session().unwrap();
        let media = crate::bus::query(read(client.inner(), name, None)).unwrap();
        assert_eq!(
            (
                media.identity.as_str(),
                media.title.as_str(),
                media.artist.as_str()
            ),
            ("Fixture Player", "Song", "Artist")
        );
        assert!(media.playing && media.can_toggle);
        let cached = crate::bus::query(read(client.inner(), name, Some("Cached"))).unwrap();
        assert_eq!(cached.identity, "Cached");
        assert!(same(
            &media,
            &crate::bus::query(read(client.inner(), name, None)).unwrap()
        ));
        assert!(!same(&media, &cached));
    }
    #[test]
    fn position_and_seek_capabilities_do_not_trigger_metadata_reads() {
        let message = |interface, keys: &[&str], invalidated: &[&str]| {
            let properties: Properties = keys
                .iter()
                .map(|key| (key.to_string(), OwnedValue::from(true)))
                .collect();
            zbus::Message::signal(PATH, "org.freedesktop.DBus.Properties", "PropertiesChanged")
                .unwrap()
                .build(&(interface, properties, invalidated.to_vec()))
                .unwrap()
        };
        for keys in [&["Position"][..], &["CanSeek"][..], &[]] {
            assert!(!visible_change(&message(PLAYER, keys, &["Position"])));
        }
        assert!(visible_change(&message(
            PLAYER,
            &["Position", "Metadata"],
            &[]
        )));
        assert!(visible_change(&message(PLAYER, &[], &["PlaybackStatus"])));
        assert!(visible_change(&message(
            "org.mpris.MediaPlayer2",
            &["Identity"],
            &[]
        )));
        assert!(!visible_change(&message(
            "org.example.Other",
            &["Metadata"],
            &[]
        )));
    }
    #[test]
    fn playing_player_wins_over_newer_paused_player_and_disappearance_falls_back() {
        let mut players = HashMap::from([
            (
                "music".into(),
                (
                    1,
                    Media {
                        bus: "music".into(),
                        title: "Track".into(),
                        playing: true,
                        ..Default::default()
                    },
                ),
            ),
            (
                "browser".into(),
                (
                    2,
                    Media {
                        bus: "browser".into(),
                        title: "Video".into(),
                        ..Default::default()
                    },
                ),
            ),
        ]);
        assert_eq!(choose(&players).bus, "music");
        players.remove("music");
        assert_eq!(choose(&players).bus, "browser");
        players.clear();
        assert!(!choose(&players).available());
    }
}
