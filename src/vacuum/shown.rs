//! What the screen shows of Spotify's things: their names and pictures
//! passed through the gate. Off, each is exactly the real one, so views
//! call these in place of reading the fields themselves. Logic (search,
//! lyrics lookups, history, scoring a game) keeps reading the real fields.

use std::borrow::Cow;

use super::gate::{self, ArtKind};
use crate::api::models::{
    Album, Artist, ArtistRef, Episode, PlayableItem, Playlist, Show, Track, User, join_names,
    pick_image,
};

/// Artists as the app lists them, each one's stand-in.
pub fn artist_names<'a>(artists: impl IntoIterator<Item = &'a ArtistRef>) -> String {
    let shown: Vec<Cow<'a, str>> = artists
        .into_iter()
        .map(|artist| gate::artist(&artist.name))
        .collect();
    join_names(shown.iter().map(|name| name.as_ref()))
}

impl Track {
    pub fn shown_name(&self) -> Cow<'_, str> {
        gate::song(&self.name)
    }

    pub fn shown_artist_names(&self) -> String {
        artist_names(&self.artists)
    }

    pub fn shown_album_name(&self) -> Cow<'_, str> {
        gate::album(self.album.as_ref().map_or("", |album| album.name.as_str()))
    }

    /// The song's picture: its album's cover, keyed by the album's ID and
    /// drawn from the song's title, as the extension keys them.
    pub fn shown_image(&self, target: u32) -> Option<String> {
        let album = self.album.as_ref();
        let id = album
            .map(|album| album.id.as_str())
            .filter(|id| !id.is_empty())
            .or(self.id.as_deref());
        let kind = if album.is_some_and(|album| !album.id.is_empty()) {
            ArtKind::Album
        } else {
            ArtKind::Track
        };
        gate::art(kind, id, &self.name, self.image(target))
    }
}

impl Album {
    pub fn shown_name(&self) -> Cow<'_, str> {
        gate::album(&self.name)
    }

    pub fn shown_artist_names(&self) -> String {
        artist_names(&self.artists)
    }

    pub fn shown_image(&self, target: u32) -> Option<String> {
        gate::art(
            ArtKind::Album,
            Some(&self.id),
            &self.name,
            pick_image(&self.images, target),
        )
    }
}

impl Artist {
    pub fn shown_name(&self) -> Cow<'_, str> {
        gate::artist(&self.name)
    }

    pub fn shown_image(&self, target: u32) -> Option<String> {
        gate::art(
            ArtKind::Artist,
            Some(&self.id),
            &self.name,
            pick_image(&self.images, target),
        )
    }
}

impl ArtistRef {
    pub fn shown_name(&self) -> Cow<'_, str> {
        gate::artist(&self.name)
    }
}

impl Playlist {
    pub fn shown_name(&self) -> Cow<'_, str> {
        gate::playlist(&self.id, self.owner.id.as_deref(), &self.name)
    }

    pub fn shown_owner_name(&self) -> Cow<'_, str> {
        gate::user(self.owner.id.as_deref(), self.owner_name())
    }

    /// The description as plain text, through the gate.
    pub fn shown_description(&self) -> Option<String> {
        self.description.as_deref().map(|text| {
            let plain = crate::util::strip_html(text);
            gate::playlist_description(&self.id, self.owner.id.as_deref(), &plain).into_owned()
        })
    }

    pub fn shown_image(&self, target: u32) -> Option<String> {
        gate::art(
            ArtKind::Playlist,
            Some(&self.id),
            &self.name,
            pick_image(&self.images, target),
        )
    }

    /// Whether the playlist may be listed at all.
    pub fn listed(&self) -> bool {
        gate::playlist_listed(&self.id, self.owner.id.as_deref())
    }
}

impl Show {
    pub fn shown_name(&self) -> Cow<'_, str> {
        gate::show(&self.name)
    }

    pub fn shown_publisher(&self) -> Cow<'_, str> {
        gate::user(None, &self.publisher)
    }

    pub fn shown_image(&self, target: u32) -> Option<String> {
        gate::art(
            ArtKind::Show,
            Some(&self.id),
            &self.name,
            pick_image(&self.images, target),
        )
    }
}

impl Episode {
    pub fn shown_name(&self) -> Cow<'_, str> {
        gate::show(&self.name)
    }

    pub fn shown_image(&self, target: u32) -> Option<String> {
        gate::art(
            ArtKind::Show,
            Some(&self.id),
            &self.name,
            pick_image(&self.images, target),
        )
    }
}

impl PlayableItem {
    pub fn shown_name(&self) -> Cow<'_, str> {
        match self {
            PlayableItem::Track(track) => track.shown_name(),
            PlayableItem::Episode(episode) => episode.shown_name(),
        }
    }

    pub fn shown_subtitle(&self) -> String {
        match self {
            PlayableItem::Track(track) => track.shown_artist_names(),
            PlayableItem::Episode(episode) => episode
                .show
                .as_ref()
                .map(|show| show.shown_name().into_owned())
                .unwrap_or_default(),
        }
    }

    pub fn shown_image(&self, target: u32) -> Option<String> {
        match self {
            PlayableItem::Track(track) => track.shown_image(target),
            PlayableItem::Episode(episode) => episode.shown_image(target),
        }
    }
}

impl User {
    pub fn shown_name(&self) -> Cow<'_, str> {
        gate::user(Some(&self.id), self.name())
    }

    pub fn shown_image(&self, target: u32) -> Option<String> {
        gate::art(
            ArtKind::User,
            Some(&self.id),
            self.name(),
            pick_image(&self.images, target),
        )
    }
}
