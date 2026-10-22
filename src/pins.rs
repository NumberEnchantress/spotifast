//! Pins shared with Spotify's own apps.
//!
//! Spotify keeps an account's pins in its library service, which its web
//! player reaches through the private Pathfinder GraphQL endpoint: the
//! `libraryV3` query marks each library item `pinned`, and the
//! `pinLibraryItem` and `unpinLibraryItem` mutations change them. These
//! requests go there the same way, with the playback session's Login5 bearer
//! and client token, as Spotifast's Home shelf does. Like that shelf, the
//! contract can change independently of the public Web API; when it fails,
//! the sidebar keeps the pins saved on this computer.
//!
//! Spotify pins only top-level items: a playlist inside a folder cannot be
//! pinned, and it caps how many pins an account has.

use serde_json::{Value, json};

use crate::settings::LIKED_SONGS_KEY;

pub(crate) const ENDPOINT: &str = "https://api-partner.spotify.com/pathfinder/v2/query";
const LIBRARY_HASH: &str = "390c78e5b951029bad359785e69b07b536a509c581cbcd0aded5e5067f187455";
/// The web player's pin and unpin mutations share one persisted document.
const PIN_HASH: &str = "896ebcb47815681340860d121cb5d494e157e2a78d3950385cd54e0393c67148";
/// Pins come first in the library, so the first page holds them all.
const PAGE: u32 = 50;
/// Liked Songs, as the library names it.
const LIKED_SONGS: &str = "spotify:collection:tracks";

/// The library's first page, as the web player asks for it, with Liked
/// Songs included so its pin is seen.
pub fn library_query() -> Value {
    json!({
        "operationName": "libraryV3",
        "variables": {
            "filters": [],
            "order": null,
            "textFilter": null,
            "features": ["LIKED_SONGS", "YOUR_EPISODES_V2", "PRERELEASES", "PRERELEASES_V2", "EVENTS"],
            "limit": PAGE,
            "offset": 0,
            "flatten": false,
            "expandedFolders": [],
            "folderUri": null,
            "includeFoldersWhenFlattening": true
        },
        "extensions": {"persistedQuery": {"version": 1, "sha256Hash": LIBRARY_HASH}}
    })
}

/// Pins or unpins one item, by the URI Spotify pins it under.
pub fn pin_query(uri: &str, pinned: bool) -> Value {
    json!({
        "operationName": if pinned { "pinLibraryItem" } else { "unpinLibraryItem" },
        "variables": {"pinnableItemUri": uri},
        "extensions": {"persistedQuery": {"version": 1, "sha256Hash": PIN_HASH}}
    })
}

fn graphql_error(response: &Value) -> Option<String> {
    let errors = response.get("errors")?.as_array()?;
    let first = errors.first()?;
    Some(
        first
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("Spotify refused the request")
            .to_string(),
    )
}

/// The library's pinned items, by URI, in Spotify's order.
pub fn pinned_uris(response: &Value) -> Result<Vec<String>, String> {
    if let Some(error) = graphql_error(response) {
        return Err(error);
    }
    let library = response
        .pointer("/data/me/libraryV3")
        .ok_or("Spotify's library answer has no library")?;
    match library.get("__typename").and_then(Value::as_str) {
        Some("LibraryPage") => {}
        other => return Err(format!("Spotify's library answered {other:?}")),
    }
    let items = library
        .get("items")
        .and_then(Value::as_array)
        .ok_or("Spotify's library answer has no items")?;
    Ok(items
        .iter()
        .filter(|item| item.get("pinned").and_then(Value::as_bool) == Some(true))
        .filter_map(|item| item.pointer("/item/_uri").and_then(Value::as_str))
        .map(str::to_string)
        .collect())
}

/// Why Spotify did not pin an item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PinRefusal {
    /// The account already has as many pins as Spotify allows.
    Limit(u32),
    /// The item sits inside a folder, where Spotify pins nothing.
    InFolder,
    /// Spotify pins only what is in the library, and not every kind of item.
    NotPinnable,
    /// The request failed, or Spotify said why in words of its own.
    Failed(String),
}

/// What Spotify said to a pin or unpin.
pub fn pin_outcome(response: &Value, pinned: bool) -> Result<(), PinRefusal> {
    if let Some(error) = graphql_error(response) {
        return Err(PinRefusal::Failed(error));
    }
    if !pinned {
        return Ok(());
    }
    let result = response
        .pointer("/data/pinItemInLibrary")
        .ok_or_else(|| PinRefusal::Failed("Spotify's pin answer is empty".into()))?;
    match result.get("pinResult").and_then(Value::as_str) {
        Some("SUCCESSFUL") => Ok(()),
        Some("FAILED_ITEM_LIMIT_REACHED") => Err(PinRefusal::Limit(
            result
                .get("pinLimit")
                .and_then(Value::as_u64)
                .map_or(4, |limit| limit as u32),
        )),
        Some("FAILED_ITEM_IN_FOLDER") => Err(PinRefusal::InFolder),
        Some("FAILED_ITEM_NOT_SUPPORTED" | "FAILED_NOT_IN_YOUR_LIBRARY") => {
            Err(PinRefusal::NotPinnable)
        }
        other => Err(PinRefusal::Failed(format!(
            "Spotify answered the pin with {other:?}"
        ))),
    }
}

/// The URI of a folder of `user`'s, as Spotify's library names it.
pub fn folder_uri(user: &str, id: &str) -> String {
    format!("spotify:user:{user}:folder:{id}")
}

/// The folder id in a library folder URI.
/// Spotify's apps sometimes append the folder's name after its id, so only
/// the id is read.
pub fn folder_id(uri: &str) -> Option<&str> {
    let (_, rest) = uri.strip_prefix("spotify:user:")?.split_once(":folder:")?;
    Some(rest.split_once(':').map_or(rest, |(id, _)| id))
}

/// The sidebar's key for an item the library names by `uri`. A folder is
/// keyed by its id, whatever else its URI carries.
pub fn key_of(uri: &str, user: &str) -> String {
    if uri == LIKED_SONGS || uri == format!("spotify:user:{user}:collection") {
        LIKED_SONGS_KEY.to_string()
    } else if let Some(id) = folder_id(uri) {
        folder_uri(user, id)
    } else {
        uri.to_string()
    }
}

/// The URI Spotify pins the item the sidebar knows by `key` under.
pub fn pinnable_uri(key: &str, user: &str) -> String {
    if key == LIKED_SONGS_KEY {
        format!("spotify:user:{user}:collection")
    } else {
        key.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pins_are_read_from_the_library_in_its_order() {
        let response = json!({"data": {"me": {"libraryV3": {
            "__typename": "LibraryPage",
            "items": [
                {"pinned": true, "item": {"__typename": "LibraryFolderResponseWrapper",
                    "_uri": "spotify:user:me:folder:aaaa"}},
                {"pinned": true, "item": {"__typename": "LibraryPseudoPlaylistResponseWrapper",
                    "_uri": "spotify:collection:tracks"}},
                {"pinned": false, "item": {"__typename": "PlaylistResponseWrapper",
                    "_uri": "spotify:playlist:x"}},
                {"pinned": true, "item": {"__typename": "PlaylistResponseWrapper",
                    "_uri": "spotify:playlist:y"}}
            ]
        }}}});
        let uris = pinned_uris(&response).unwrap();
        assert_eq!(
            uris,
            [
                "spotify:user:me:folder:aaaa",
                "spotify:collection:tracks",
                "spotify:playlist:y"
            ]
        );
        let keys: Vec<_> = uris.iter().map(|uri| key_of(uri, "me")).collect();
        assert_eq!(keys[1], LIKED_SONGS_KEY);
        assert_eq!(folder_id(&keys[0]), Some("aaaa"));
    }

    #[test]
    fn a_library_error_is_not_read_as_no_pins() {
        for response in [
            json!({"errors": [{"message": "PersistedQueryNotFound"}]}),
            json!({"data": {"me": {"libraryV3": {"__typename": "NotFound"}}}}),
            json!({"data": {}}),
        ] {
            assert!(pinned_uris(&response).is_err(), "{response}");
        }
    }

    #[test]
    fn spotify_says_why_a_pin_was_refused() {
        let answer = |result: &str| json!({"data": {"pinItemInLibrary": {"pinResult": result, "pinLimit": 4}}});
        assert_eq!(pin_outcome(&answer("SUCCESSFUL"), true), Ok(()));
        assert_eq!(
            pin_outcome(&answer("FAILED_ITEM_LIMIT_REACHED"), true),
            Err(PinRefusal::Limit(4))
        );
        assert_eq!(
            pin_outcome(&answer("FAILED_ITEM_IN_FOLDER"), true),
            Err(PinRefusal::InFolder)
        );
        assert_eq!(
            pin_outcome(&answer("FAILED_NOT_IN_YOUR_LIBRARY"), true),
            Err(PinRefusal::NotPinnable)
        );
        assert!(matches!(
            pin_outcome(&json!({"errors": [{"message": "nope"}]}), false),
            Err(PinRefusal::Failed(_))
        ));
        assert_eq!(pin_outcome(&json!({"data": {}}), false), Ok(()));
    }

    #[test]
    fn requests_name_what_the_web_player_names() {
        let query = pin_query("spotify:user:me:collection", true);
        assert_eq!(query["operationName"], "pinLibraryItem");
        assert_eq!(
            query["variables"]["pinnableItemUri"],
            "spotify:user:me:collection"
        );
        assert_eq!(pin_query("x", false)["operationName"], "unpinLibraryItem");
        assert_eq!(library_query()["operationName"], "libraryV3");
        assert_eq!(
            pinnable_uri(LIKED_SONGS_KEY, "me"),
            "spotify:user:me:collection"
        );
        assert_eq!(
            pinnable_uri("spotify:playlist:x", "me"),
            "spotify:playlist:x"
        );
        assert_eq!(folder_uri("me", "ab12"), "spotify:user:me:folder:ab12");
    }

    /// Liked Songs reads back as pinned whichever URI names it, and a folder
    /// is found by its id whatever trails it.
    #[test]
    fn items_read_back_under_the_sidebar_s_keys() {
        assert_eq!(key_of("spotify:user:me:collection", "me"), LIKED_SONGS_KEY);
        assert_eq!(key_of("spotify:collection:tracks", "me"), LIKED_SONGS_KEY);
        assert_eq!(
            key_of("spotify:user:me:folder:ab12:Road+Trip", "me"),
            "spotify:user:me:folder:ab12"
        );
        assert_eq!(folder_id("spotify:user:me:folder:ab12"), Some("ab12"));
        assert_eq!(key_of("spotify:playlist:x", "me"), "spotify:playlist:x");
    }

    #[test]
    fn an_answer_spotify_has_no_name_for_is_not_a_pin() {
        let answer = json!({"data": {"pinItemInLibrary": {"pinResult": "SOMETHING_NEW"}}});
        assert!(matches!(
            pin_outcome(&answer, true),
            Err(PinRefusal::Failed(_))
        ));
    }
}
