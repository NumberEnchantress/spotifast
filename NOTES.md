# Notes

Decisions for this fork's own features, with dates, so later work follows
them. The product and contribution rules stay in `CONTRIBUTING.md` and
`AGENTS.md`.

## Vacuum (Unfamiliar Sounds in the app)

The Unfamiliar Sounds Firefox extension, brought into the app as Vacuum.
The reference implementation is the extension itself; its handoff notes
record its own decisions and three rounds of adversarial grading.

### 29 October 2026

- **Name:** the feature is called Vacuum. The fixed strings inside the
  derivation (`unfamiliar sounds seed:`, `picture:`) keep the extension's
  name, or stand-ins would stop matching.
- **Parity:** feature parity with the extension is the goal. Browser-only
  parts have no counterpart: the container, closing tabs when switching
  off, and asking how to open links. The toolbar button and Alt+Shift+D
  become the account menu's Vacuum item and the same shortcut. A top-bar
  button was tried and left out: with it, the search field fell below its
  readable width in the narrowest window.
- **Derivation:** ported exactly from `core/derive.js` and `core/museums.js`.
  `src/vacuum/golden-vectors.json` holds the extension's own outputs for two
  test seeds; every value is checked by a test. Names are keyed by the name
  shown, pictures by Spotify ID. Songs, podcasts, episodes and audiobooks
  are named in the album name space, as the extension does.
- **Shared seed:** committed, lightly scrambled (`src/vacuum/shared_seed.rs`),
  as decided on 29 October 2026. It is never shown, logged, put in settings
  or docs, or printed by a test. It ships in the app, as it does in the
  extension.
- **Design:** names and pictures are replaced where they are drawn (the
  brief's design b). The app keeps its real data, so search, scoring a game
  and switching off need nothing re-fetched. Views call `shown_*` helpers
  (`src/vacuum/shown.rs`) or the gate (`src/vacuum/gate.rs`); logic keeps
  the real fields. The gate fails closed: a name it cannot derive is shown
  as `???`.
- **Levels:** the gate has two levels besides off: Disguised (Vacuum) and
  Blanked (`???` and black squares), for the guessing game.
- **Years:** release years and dates stay real, as in the extension.
- **Lyrics:** shown with the names Vacuum knows swapped.
- **Approved by the user:** the museum requests (The Met, the Art Institute
  of Chicago, the Cleveland Museum of Art and their image hosts) and the
  `unicode-normalization` crate. `regex` and `resvg` were already built for
  other crates.
- **Leak scan:** `demo::tests::vacuum_shows_no_real_name_or_picture_anywhere`
  walks every page and panel of the demo with Vacuum on and everything of
  your own replaced too, and fails on any real name drawn, given to the
  screen reader, put in the window title, the media controls or the command
  line, or any real picture asked for.

### Guessing game, 29 October 2026

- **Decisions (the user):** a song without synced lyrics is guessed until a
  fifth of the way in; a game plays from a source picked per game; the
  stats show after 10 songs; points are independent per category (title
  1, artist 2, album 3, year 4, version 5).
- **Mine, from the briefs:** Name it defaults to Blanked; only typed rounds
  count in the stats; a song joined late (more than 5 seconds in) gets 30
  seconds; a miss is the title missed, or the wrong cover or colour; it
  comes back after a day, then, got right each time, after 3 and 7 days, and
  plays first. A miss when due starts it over; a miss before it is due
  changes nothing. A round skipped or ended before its reveal is not kept,
  nor one that cannot be scored (no cover read, too few covers). Numbers in
  typed answers must be exact; a version's words may come in any order. The version is read from Spotify's title, never
  guessed. The cover grid offers covers of other albums in the game, and the
  main colour is the largest of twelve colour bins over a small copy of the
  cover, a close second counting too.
- **Hiding:** the game puts Vacuum's gate at its level while a song is
  guessed, and while a song of the game not yet guessed plays before its
  round begins (so Next shows nothing real for a frame), Vacuum on or off,
  and lets it go at the reveal. Blanked hides every name in the app, not
  only the playing song's. The queue is hidden for the whole game unless the
  game shows real names. With Vacuum off, a blanked game's gate is light: it
  learns and reads nothing, and the search box keeps what was typed. A song
  outside the game, or one guessed already (Previous), is not hidden or
  guessed. Shuffle is turned off for the game and given back at its end.
- **Lyrics:** the game asks for each song's lyrics, panel open or not, to
  time the deadline; how-it-connects says so.
- **Kept:** `game.json` in the state folder; the game's setup is kept there,
  not as a setting.

### Still open

- Guessing game: with Vacuum off and the game showing stand-ins, the gate is
  built at each round and let go at each reveal, learning the library again
  each time; a blanked game, and any game with Vacuum on, do not. An Honor
  round revealed but not moved past is lost if the app quits, and shuffle
  stays off if the app quits mid-game. A song librespot skips as unplayable
  stays to be played, so Continue the game offers it again.

- Playlist verdicts are not kept between runs; a playlist judged about a
  person from its songs is judged again from its title next time.
- Spotify's own format for its playlists (artist mixes, charts) is not read,
  so rule 0 of the extension's judgement does not apply; the title rules
  catch the same mixes by their titles.
- Save as playlist on a radio, and Save queue as playlist, name the new
  playlist after the real station or date. It is then your own playlist,
  which keeps its title unless Replace my own playlists is on.
- Edit details on your own playlist shows its real name, description and
  cover, even with Replace my own playlists on, as editing needs them.
- The app shows no artist biographies, so Hide artist descriptions has
  nothing to hide yet; it is kept for parity and for when it does.
- Accent colours from covers are off while Vacuum is on: reading them from
  a real cover would tell of it.
- A grader's report (29 October 2026) found leaks the first leak scan
  missed; all fixed, and the scan now also visits Settings, a wide window
  (Added by), the Add to playlist list and a search by stand-in.
