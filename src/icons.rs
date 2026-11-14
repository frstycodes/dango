use std::borrow::Cow;

use gpui_kit::{AssetSource, Result, SharedString, assets::IconNamed};

pub const CLYDE: &str = "brand/discord-symbol-blurple.svg";
pub const CLYDE_WHITE: &str = "brand/discord-symbol-white.svg";

// Discord's own icons, extracted from its web app for the user's personal use (assets/discord/README.md:
// never redistribute). Order matches `Glyph`.
const GLYPHS: [(&str, &[u8]); 54] = [
    ("icons/hashtag.svg", include_bytes!("../assets/discord/icons/hashtag.svg")),
    ("icons/hashtag-lock.svg", include_bytes!("../assets/discord/icons/hashtag-lock.svg")),
    ("icons/volume-high.svg", include_bytes!("../assets/discord/icons/volume-high.svg")),
    ("icons/chat-round.svg", include_bytes!("../assets/discord/icons/chat-round.svg")),
    ("icons/user.svg", include_bytes!("../assets/discord/icons/user.svg")),
    ("icons/mic.svg", include_bytes!("../assets/discord/icons/mic.svg")),
    ("icons/mic-off.svg", include_bytes!("../assets/discord/icons/mic-off.svg")),
    ("icons/headphones.svg", include_bytes!("../assets/discord/icons/headphones.svg")),
    ("icons/headphones-off.svg", include_bytes!("../assets/discord/icons/headphones-off.svg")),
    ("icons/settings.svg", include_bytes!("../assets/discord/icons/settings.svg")),
    ("icons/users.svg", include_bytes!("../assets/discord/icons/users.svg")),
    ("icons/reply.svg", include_bytes!("../assets/discord/icons/reply.svg")),
    ("icons/pen.svg", include_bytes!("../assets/discord/icons/pen.svg")),
    ("icons/trash.svg", include_bytes!("../assets/discord/icons/trash.svg")),
    ("icons/x.svg", include_bytes!("../assets/discord/icons/x.svg")),
    ("icons/plus.svg", include_bytes!("../assets/discord/icons/plus.svg")),
    ("icons/chevron-down.svg", include_bytes!("../assets/discord/icons/chevron-down.svg")),
    ("icons/search.svg", include_bytes!("../assets/discord/icons/search.svg")),
    ("icons/arrow-left.svg", include_bytes!("../assets/discord/icons/arrow-left.svg")),
    ("icons/arrow-right.svg", include_bytes!("../assets/discord/icons/arrow-right.svg")),
    ("icons/folder.svg", include_bytes!("../assets/discord/icons/folder.svg")),
    ("icons/copy.svg", include_bytes!("../assets/discord/icons/copy.svg")),
    ("icons/play.svg", include_bytes!("../assets/discord/icons/play.svg")),
    ("icons/open.svg", include_bytes!("../assets/discord/icons/open.svg")),
    ("icons/chevron-left.svg", include_bytes!("../assets/discord/icons/chevron-left.svg")),
    ("icons/chevron-right.svg", include_bytes!("../assets/discord/icons/chevron-right.svg")),
    ("icons/checkmark.svg", include_bytes!("../assets/discord/icons/checkmark.svg")),
    ("icons/call-end.svg", include_bytes!("../assets/discord/icons/phone-hang-up.svg")),
    ("icons/share-screen.svg", include_bytes!("../assets/discord/icons/share-screen.svg")),
    ("icons/emoji.svg", include_bytes!("../assets/discord/icons/emoji.svg")),
    ("icons/emoji-add.svg", include_bytes!("../assets/discord/icons/emoji-add.svg")),
    ("icons/game.svg", include_bytes!("../assets/discord/icons/game.svg")),
    ("icons/music.svg", include_bytes!("../assets/discord/icons/music.svg")),
    ("icons/stream.svg", include_bytes!("../assets/discord/icons/stream.svg")),
    ("icons/tv.svg", include_bytes!("../assets/discord/icons/tv.svg")),
    ("icons/pin.svg", include_bytes!("../assets/discord/icons/pin.svg")),
    ("icons/friends.svg", include_bytes!("../assets/discord/icons/friends.svg")),
    ("icons/inbox.svg", include_bytes!("../assets/discord/icons/inbox.svg")),
    ("icons/help.svg", include_bytes!("../assets/discord/icons/help.svg")),
    ("icons/gif.svg", include_bytes!("../assets/discord/icons/gif.svg")),
    ("icons/bell.svg", include_bytes!("../assets/discord/icons/bell.svg")),
    ("icons/keyboard.svg", include_bytes!("../assets/discord/icons/keyboard.svg")),
    ("icons/video.svg", include_bytes!("../assets/discord/icons/video.svg")),
    ("icons/video-off.svg", include_bytes!("../assets/discord/icons/video-off.svg")),
    ("icons/connection-fine.svg", include_bytes!("../assets/discord/icons/connection-fine.svg")),
    ("icons/connection-average.svg", include_bytes!("../assets/discord/icons/connection-average.svg")),
    ("icons/connection-bad.svg", include_bytes!("../assets/discord/icons/connection-bad.svg")),
    ("icons/connection-unknown.svg", include_bytes!("../assets/discord/icons/connection-unknown.svg")),
    ("icons/palette.svg", include_bytes!("../assets/discord/icons/palette.svg")),
    ("icons/shield-lock.svg", include_bytes!("../assets/discord/icons/shield-lock.svg")),
    ("icons/waveform.svg", include_bytes!("../assets/discord/icons/waveform.svg")),
    ("icons/screen.svg", include_bytes!("../assets/discord/icons/screen.svg")),
    ("icons/screen-off.svg", include_bytes!("../assets/discord/icons/screen-off.svg")),
    ("icons/door-exit.svg", include_bytes!("../assets/discord/icons/door-exit.svg")),
];

// Discord's symbol (Clyde) in its blurple and white versions, unmodified from discord.com/branding.
const BRAND: [(&str, &[u8]); 2] = [
    (CLYDE, include_bytes!("../assets/brand/discord-symbol-blurple.svg")),
    (CLYDE_WHITE, include_bytes!("../assets/brand/discord-symbol-white.svg")),
];

#[derive(Clone, Copy)]
pub enum Glyph {
    Hashtag,
    HashtagLock,
    VolumeHigh,
    ChatRound,
    User,
    Mic,
    MicOff,
    Headphones,
    HeadphonesOff,
    Settings,
    Users,
    Reply,
    Pen,
    Trash,
    X,
    Plus,
    ChevronDown,
    Search,
    ArrowLeft,
    ArrowRight,
    Folder,
    Copy,
    Play,
    Open,
    ChevronLeft,
    ChevronRight,
    Checkmark,
    CallEnd,
    ShareScreen,
    Emoji,
    EmojiAdd,
    Game,
    Music,
    Stream,
    Tv,
    Pin,
    Friends,
    Inbox,
    Help,
    Gif,
    Bell,
    Keyboard,
    Video,
    VideoOff,
    ConnectionFine,
    ConnectionAverage,
    ConnectionBad,
    ConnectionUnknown,
    Palette,
    ShieldLock,
    Waveform,
    Screen,
    ScreenOff,
    DoorExit,
}

impl IconNamed for Glyph {
    fn path(self) -> SharedString {
        GLYPHS[self as usize].0.into()
    }
}

// Serves Rustcord's glyphs and brand art, then falls back to gpui-kit's own assets so its components keep their icons.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        match GLYPHS.iter().chain(BRAND.iter()).find(|(p, _)| *p == path) {
            Some((_, bytes)) => Ok(Some(Cow::Borrowed(bytes))),
            None => gpui_kit::assets::Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = gpui_kit::assets::Assets.list(path)?;
        paths.extend(GLYPHS.iter().chain(BRAND.iter()).map(|(p, _)| *p).filter(|p| p.starts_with(path)).map(SharedString::from));
        Ok(paths)
    }
}
