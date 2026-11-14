use std::borrow::Cow;

use gpui_kit::{AssetSource, Result, SharedString, assets::IconNamed};

// Wave's own mark, the voice wave, in its magenta and in white.
pub const MARK: &str = "brand/rustcord-mark.svg";
pub const MARK_WHITE: &str = "brand/rustcord-mark-white.svg";

// Phosphor icons (MIT, assets/icons/phosphor/LICENSE): regular for rows and chrome, bold for small
// marks, fill for on and playing states. Order matches `Glyph`.
const GLYPHS: [(&str, &[u8]); 80] = [
    ("icons/hashtag.svg", include_bytes!("../assets/icons/phosphor/hash.svg")),
    ("icons/hashtag-lock.svg", include_bytes!("../assets/icons/phosphor/lock-simple.svg")),
    ("icons/volume-high.svg", include_bytes!("../assets/icons/phosphor/speaker-high.svg")),
    ("icons/chat-round.svg", include_bytes!("../assets/icons/phosphor/chat-circle.svg")),
    ("icons/user.svg", include_bytes!("../assets/icons/phosphor/user.svg")),
    ("icons/mic.svg", include_bytes!("../assets/icons/phosphor/microphone.svg")),
    ("icons/mic-off.svg", include_bytes!("../assets/icons/phosphor/microphone-slash.svg")),
    ("icons/headphones.svg", include_bytes!("../assets/icons/phosphor/headphones.svg")),
    ("icons/headphones-off.svg", include_bytes!("../assets/icons/phosphor/ear-slash.svg")),
    ("icons/settings.svg", include_bytes!("../assets/icons/phosphor/gear-six.svg")),
    ("icons/users.svg", include_bytes!("../assets/icons/phosphor/users.svg")),
    ("icons/reply.svg", include_bytes!("../assets/icons/phosphor/arrow-bend-up-left.svg")),
    ("icons/pen.svg", include_bytes!("../assets/icons/phosphor/pencil-simple.svg")),
    ("icons/trash.svg", include_bytes!("../assets/icons/phosphor/trash.svg")),
    ("icons/x.svg", include_bytes!("../assets/icons/phosphor/x.svg")),
    ("icons/plus.svg", include_bytes!("../assets/icons/phosphor/plus.svg")),
    ("icons/chevron-down.svg", include_bytes!("../assets/icons/phosphor/caret-down.svg")),
    ("icons/search.svg", include_bytes!("../assets/icons/phosphor/magnifying-glass.svg")),
    ("icons/arrow-left.svg", include_bytes!("../assets/icons/phosphor/arrow-left.svg")),
    ("icons/arrow-right.svg", include_bytes!("../assets/icons/phosphor/arrow-right.svg")),
    ("icons/folder.svg", include_bytes!("../assets/icons/phosphor/folder-simple-fill.svg")),
    ("icons/copy.svg", include_bytes!("../assets/icons/phosphor/copy.svg")),
    ("icons/play.svg", include_bytes!("../assets/icons/phosphor/play-fill.svg")),
    ("icons/open.svg", include_bytes!("../assets/icons/phosphor/arrow-square-out.svg")),
    ("icons/chevron-left.svg", include_bytes!("../assets/icons/phosphor/caret-left.svg")),
    ("icons/chevron-right.svg", include_bytes!("../assets/icons/phosphor/caret-right.svg")),
    ("icons/checkmark.svg", include_bytes!("../assets/icons/phosphor/check-bold.svg")),
    ("icons/call-end.svg", include_bytes!("../assets/icons/phosphor/phone-x-fill.svg")),
    ("icons/share-screen.svg", include_bytes!("../assets/icons/phosphor/screencast.svg")),
    ("icons/emoji.svg", include_bytes!("../assets/icons/phosphor/smiley.svg")),
    ("icons/emoji-add.svg", include_bytes!("../assets/icons/phosphor/smiley-sticker.svg")),
    ("icons/game.svg", include_bytes!("../assets/icons/phosphor/game-controller.svg")),
    ("icons/music.svg", include_bytes!("../assets/icons/phosphor/music-notes.svg")),
    ("icons/stream.svg", include_bytes!("../assets/icons/phosphor/broadcast.svg")),
    ("icons/tv.svg", include_bytes!("../assets/icons/phosphor/television-simple.svg")),
    ("icons/pin.svg", include_bytes!("../assets/icons/phosphor/push-pin.svg")),
    ("icons/friends.svg", include_bytes!("../assets/icons/phosphor/users-three.svg")),
    ("icons/inbox.svg", include_bytes!("../assets/icons/phosphor/tray.svg")),
    ("icons/help.svg", include_bytes!("../assets/icons/phosphor/question.svg")),
    ("icons/gif.svg", include_bytes!("../assets/icons/phosphor/gif.svg")),
    ("icons/bell.svg", include_bytes!("../assets/icons/phosphor/bell.svg")),
    ("icons/keyboard.svg", include_bytes!("../assets/icons/phosphor/keyboard.svg")),
    ("icons/video.svg", include_bytes!("../assets/icons/phosphor/video-camera.svg")),
    ("icons/video-off.svg", include_bytes!("../assets/icons/phosphor/video-camera-slash.svg")),
    ("icons/connection-fine.svg", include_bytes!("../assets/icons/phosphor/cell-signal-full-fill.svg")),
    ("icons/connection-average.svg", include_bytes!("../assets/icons/phosphor/cell-signal-medium-fill.svg")),
    ("icons/connection-bad.svg", include_bytes!("../assets/icons/phosphor/cell-signal-low-fill.svg")),
    ("icons/connection-unknown.svg", include_bytes!("../assets/icons/phosphor/cell-signal-x-fill.svg")),
    ("icons/palette.svg", include_bytes!("../assets/icons/phosphor/palette.svg")),
    ("icons/shield-lock.svg", include_bytes!("../assets/icons/phosphor/shield-check.svg")),
    ("icons/waveform.svg", include_bytes!("../assets/icons/phosphor/waveform.svg")),
    ("icons/screen.svg", include_bytes!("../assets/icons/phosphor/monitor.svg")),
    ("icons/screen-off.svg", include_bytes!("../assets/icons/phosphor/prohibit.svg")),
    ("icons/door-exit.svg", include_bytes!("../assets/icons/phosphor/sign-out.svg")),
    ("icons/crown.svg", include_bytes!("../assets/icons/phosphor/crown-simple-fill.svg")),
    ("icons/id.svg", include_bytes!("../assets/icons/phosphor/identification-card.svg")),
    ("icons/more.svg", include_bytes!("../assets/icons/phosphor/dots-three-bold.svg")),
    ("icons/thread.svg", include_bytes!("../assets/icons/phosphor/chats-circle.svg")),
    ("icons/forum.svg", include_bytes!("../assets/icons/phosphor/chats.svg")),
    ("icons/soundboard.svg", include_bytes!("../assets/icons/phosphor/speaker-simple-high.svg")),
    ("icons/activities.svg", include_bytes!("../assets/icons/phosphor/rocket-launch.svg")),
    ("icons/fullscreen.svg", include_bytes!("../assets/icons/phosphor/corners-out.svg")),
    ("icons/fullscreen-exit.svg", include_bytes!("../assets/icons/phosphor/corners-in.svg")),
    ("icons/chevron-up.svg", include_bytes!("../assets/icons/phosphor/caret-up.svg")),
    ("icons/group-plus.svg", include_bytes!("../assets/icons/phosphor/user-plus.svg")),
    ("icons/pause.svg", include_bytes!("../assets/icons/phosphor/pause-fill.svg")),
    ("icons/volume-off.svg", include_bytes!("../assets/icons/phosphor/speaker-slash.svg")),
    ("icons/eye.svg", include_bytes!("../assets/icons/phosphor/eye.svg")),
    ("icons/paper-plane.svg", include_bytes!("../assets/icons/phosphor/paper-plane-tilt-bold.svg")),
    ("icons/upload.svg", include_bytes!("../assets/icons/phosphor/upload-simple.svg")),
    ("icons/image.svg", include_bytes!("../assets/icons/phosphor/image.svg")),
    ("icons/link.svg", include_bytes!("../assets/icons/phosphor/link.svg")),
    ("icons/file.svg", include_bytes!("../assets/icons/phosphor/file.svg")),
    ("icons/warning-circle.svg", include_bytes!("../assets/icons/phosphor/warning-circle-fill.svg")),
    ("icons/minus.svg", include_bytes!("../assets/icons/phosphor/minus-bold.svg")),
    ("icons/info.svg", include_bytes!("../assets/icons/phosphor/info-fill.svg")),
    ("icons/warning.svg", include_bytes!("../assets/icons/phosphor/warning-fill.svg")),
    ("icons/x-circle.svg", include_bytes!("../assets/icons/phosphor/x-circle-fill.svg")),
    ("icons/check-circle.svg", include_bytes!("../assets/icons/phosphor/check-circle-fill.svg")),
    ("icons/at.svg", include_bytes!("../assets/icons/phosphor/at-fill.svg")),
];

const BRAND: [(&str, &[u8]); 2] = [
    (MARK, include_bytes!("../assets/brand/rustcord-mark.svg")),
    (MARK_WHITE, include_bytes!("../assets/brand/rustcord-mark-white.svg")),
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
    Crown,
    Id,
    More,
    Thread,
    Forum,
    Soundboard,
    Activities,
    Fullscreen,
    FullscreenExit,
    ChevronUp,
    GroupPlus,
    Pause,
    VolumeOff,
    Eye,
    PaperPlane,
    Upload,
    Image,
    Link,
    File,
    WarningCircle,
    Minus,
    Info,
    Warning,
    XCircle,
    CheckCircle,
    At,
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
        match GLYPHS.iter().chain(BRAND.iter()).chain(crate::app_icon::BRAND_ICONS.iter()).find(|(p, _)| *p == path) {
            Some((_, bytes)) => Ok(Some(Cow::Borrowed(bytes))),
            None => gpui_kit::assets::Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = gpui_kit::assets::Assets.list(path)?;
        paths.extend(GLYPHS.iter().chain(BRAND.iter()).chain(crate::app_icon::BRAND_ICONS.iter()).map(|(p, _)| *p).filter(|p| p.starts_with(path)).map(SharedString::from));
        Ok(paths)
    }
}
