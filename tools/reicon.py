"""Builds design/icons-data.js for design/icons.html: every icon Dango draws today (Rust glyphs, sidebar
glyphs and the spec's Phosphor names), mapped to its Reicon replacement, with the Reicon SVGs it needs.

    npm pack @iconify-json/reicon@1.2.6 && tar xzf iconify-json-reicon-1.2.6.tgz
    python3 tools/reicon.py package/icons.json

Reicon (MIT, github.com/dqev/reicon) draws at 1.5 on a 24 grid; the bodies are thickened to ~2 here:
stroke-drawn ones get stroke-width 2, outlined ones (filled shapes) a 0.5 stroke painted under the fill.
"""

import json
import re
import sys
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# (current Phosphor name or Rust variant, group, meaning, Reicon name | CUSTOM:<id> | BRAND | GAP)
MAP = [
    ("hash", "Channels", "Text channel", "hashtag2"),
    ("lock-simple", "Channels", "Locked", "lock2"),
    ("speaker-high", "Channels", "Voice channel", "volume-high"),
    ("megaphone", "Channels", "Announcement channel", "bullhorn"),
    ("megaphone-simple", "Channels", "Announcement", "bullhorn"),
    ("microphone-stage", "Channels", "Stage channel", "podcast"),
    ("chats", "Channels", "Forum channel", "messages2"),
    ("images", "Channels", "Media channel", "gallery"),
    ("chats-circle", "Channels", "Thread", "messages3"),
    ("chats-teardrop", "Channels", "New thread", "message-add"),
    ("chat-circle", "Channels", "Direct message", "chat-round"),
    ("chat-circle-text", "Channels", "Message", "chat-round-line"),
    ("chat-circle-dots", "Channels", "Typing", "chat-round-dots"),
    ("chat-teardrop-text", "Channels", "Reply in thread", "message-text"),
    ("chat-text", "Channels", "Text chat in call", "message-text2"),
    ("folder-simple", "Channels", "Server folder", "folder"),
    ("folder", "Channels", "Folder", "folder"),
    ("folder-plus", "Channels", "New folder", "folder-plus"),
    ("folder-simple-plus", "Channels", "New folder", "folder-add"),
    ("folder-minus", "Channels", "Remove from folder", "folder-minus"),
    ("folder-simple-dashed", "Channels", "Empty folder", "folder-2"),
    ("compass", "Channels", "Discover", "compass"),
    ("tray", "Channels", "Inbox", "inbox"),
    ("tray-arrow-down", "Channels", "Archived inbox", "inbox-in"),
    ("microphone", "Voice", "Microphone", "microphone2"),
    ("microphone-slash", "Voice", "Muted", "CUSTOM:muted"),
    ("headphones", "Voice", "Headphones", "headphone"),
    ("ear-slash", "Voice", "Deafened", "CUSTOM:deafened"),
    ("speaker-slash", "Voice", "Speaker off", "volume-slash"),
    ("speaker-simple-high", "Voice", "Soundboard", "music-square"),
    ("speaker-simple-slash", "Voice", "Soundboard off", "music-square-remove"),
    ("speaker-low", "Voice", "Volume low", "volume-low"),
    ("speaker-none", "Voice", "Volume silent", "volume"),
    ("speaker-x", "Voice", "Volume muted", "volume-x"),
    ("waveform", "Voice", "Voice activity", "soundwave"),
    ("screencast", "Voice", "Share screen", "screencast"),
    ("monitor", "Voice", "Screen", "monitor"),
    ("monitor-play", "Voice", "Watch stream", "presentation-play"),
    ("broadcast", "Voice", "Live", "signal-stream"),
    ("video-camera", "Voice", "Camera", "video"),
    ("video-camera-slash", "Voice", "Camera off", "CUSTOM:camera-off"),
    ("camera", "Voice", "Photo camera", "camera"),
    ("phone", "Voice", "Call", "phone"),
    ("phone-call", "Voice", "Ringing", "phone-ring"),
    ("phone-x", "Voice", "Leave call", "phone-hangup"),
    ("phone-disconnect", "Voice", "Disconnect", "phone-off"),
    ("rocket-launch", "Voice", "Activities", "rocket"),
    ("picture-in-picture", "Voice", "Picture in picture", "pip"),
    ("corners-out", "Voice", "Full screen", "fullscreen"),
    ("corners-in", "Voice", "Exit full screen", "exit-fullscreen"),
    ("arrows-out-simple", "Voice", "Pop out", "maximize4"),
    ("arrows-in-simple", "Voice", "Pop in", "minimize"),
    ("square-split-horizontal", "Voice", "Split view", "sidebar-right"),
    ("record", "Voice", "Record", "record"),
    ("stop", "Voice", "Stop", "stop"),
    ("stop-circle", "Voice", "Stop recording", "stop-circle"),
    ("play", "Voice", "Play", "play"),
    ("pause", "Voice", "Pause", "pause"),
    ("pause-circle", "Voice", "Paused", "pause-circle"),
    ("app-window", "Voice", "Window", "window"),
    ("usb", "Voice", "Capture device", "usb"),
    ("game-controller", "Voice", "Game", "gamepad"),
    ("joystick", "Voice", "Playing", "gamepad2"),
    ("music-notes", "Voice", "Listening", "music-notes"),
    ("television-simple", "Voice", "Watching", "tv"),
    ("arrow-bend-up-left", "Messages", "Reply", "reply"),
    ("arrow-bend-up-right", "Messages", "Forward", "forward"),
    ("arrow-bend-down-right", "Messages", "Reply line", "CUSTOM:reply-spine"),
    ("arrow-elbow-down-right", "Messages", "Move to", "arrow-right-down"),
    ("arrow-elbow-down-left", "Messages", "Return", "arrow-left-down"),
    ("arrow-elbow-left-up", "Messages", "Jump to reply", "arrow-left-up"),
    ("arrow-u-up-left", "Messages", "Undo", "undo"),
    ("pencil-simple", "Messages", "Edit", "pen"),
    ("note-pencil", "Messages", "Note", "note-text"),
    ("trash", "Messages", "Delete", "trash4"),
    ("copy", "Messages", "Copy", "copy"),
    ("copy-simple", "Messages", "Copy text", "copy3"),
    ("link", "Messages", "Copy link", "link"),
    ("link-simple", "Messages", "Link", "link2"),
    ("link-break", "Messages", "Broken link", "link-broken"),
    ("link-simple-break", "Messages", "Unlink", "link-off"),
    ("push-pin", "Messages", "Pin", "pin"),
    ("push-pin-simple", "Messages", "Pinned", "pin"),
    ("push-pin-slash", "Messages", "Unpin", "pin-off"),
    ("smiley", "Messages", "Emoji", "emoji-happy"),
    ("smiley-sticker", "Messages", "Add reaction", "face-plus"),
    ("sticker", "Messages", "Sticker", "sticker-smile"),
    ("smiley-x-eyes", "Messages", "Remove reactions", "emoji-sad"),
    ("smiley-sad", "Messages", "Sad", "sad-circle"),
    ("gif", "Messages", "GIF", "CUSTOM:gif"),
    ("plus-circle", "Messages", "Attach", "add-circle"),
    ("paperclip", "Messages", "Attachment", "paperclip"),
    ("paper-plane-tilt", "Messages", "Send", "send2"),
    ("at", "Messages", "Mention", "at-sign"),
    ("envelope-simple", "Messages", "Mark unread", "envelope-unread"),
    ("checks", "Messages", "Mark read", "check-read"),
    ("clock", "Messages", "Sent later", "clock"),
    ("clock-countdown", "Messages", "Slow mode", "timer-start"),
    ("timer", "Messages", "Timeout", "timer"),
    ("hourglass", "Messages", "Waiting", "hourglass"),
    ("hourglass-medium", "Messages", "Pending", "hourglass2"),
    ("flag", "Messages", "Report", "flag"),
    ("flag-banner", "Messages", "Banner", "flag-banner"),
    ("scroll", "Messages", "Rules", "scroll"),
    ("book-open-text", "Messages", "Guide", "book-open"),
    ("books", "Messages", "Library", "library"),
    ("code", "Messages", "Code", "code"),
    ("text-aa", "Messages", "Text format", "text"),
    ("eraser", "Messages", "Clear", "eraser"),
    ("line-segment", "Messages", "Divider", "minus"),
    ("list-checks", "Messages", "Checklist", "list-check"),
    ("list-magnifying-glass", "Messages", "Search results", "search-status"),
    ("clipboard-text", "Messages", "Paste", "clipboard-text"),
    ("qr-code", "Messages", "QR code", "qr"),
    ("user", "People", "Person", "user"),
    ("user-circle", "People", "Profile", "user-circle"),
    ("user-circle-gear", "People", "Edit profile", "user-edit"),
    ("user-circle-plus", "People", "Add account", "user-cirlce-add"),
    ("user-circle-dashed", "People", "Unknown person", "profile-circle"),
    ("user-plus", "People", "Invite", "user-add"),
    ("user-minus", "People", "Remove member", "user-minus"),
    ("user-check", "People", "Friend", "user-check"),
    ("user-switch", "People", "Switch account", "arrow-swap-horizontal"),
    ("user-focus", "People", "Focus on person", "user-scan"),
    ("user-square", "People", "Avatar", "user-square"),
    ("users", "People", "Members", "users"),
    ("users-three", "People", "Group", "users2"),
    ("users-four", "People", "Community", "group"),
    ("identification-card", "People", "Copy ID", "user-id"),
    ("identification-badge", "People", "Role", "personalcard"),
    ("crown-simple", "People", "Owner", "crown"),
    ("shield", "People", "Moderator", "shield"),
    ("shield-check", "People", "Verified", "shield-tick"),
    ("shield-star", "People", "Admin", "shield-star"),
    ("shield-warning", "People", "Safety alert", "shield-alert"),
    ("shield-slash", "People", "No protection", "shield-slash"),
    ("gavel", "People", "Ban", "judge"),
    ("door-open", "People", "Kick", "exit"),
    ("hand-waving", "People", "Wave hello", "hand"),
    ("hand", "People", "Raise hand", "hand"),
    ("hand-palm", "People", "Request to speak", "hand"),
    ("hand-pointing", "People", "Click", "pointer"),
    ("hand-grabbing", "People", "Dragging", "hand"),
    ("signature", "People", "Nickname", "signpost"),
    ("tag", "People", "Tag", "tag"),
    ("seal-check", "People", "Verified server", "verified"),
    ("heart", "People", "Favourite", "heart"),
    ("heart-break", "People", "Unfriend", "heart-broken"),
    ("student", "People", "Student hub", "graduation-cap"),
    ("baby", "People", "Age gate", "happyemoji"),
    ("caret-down", "Chrome", "Disclosure open", "chevron-down"),
    ("caret-right", "Chrome", "Disclosure", "chevron-right"),
    ("caret-left", "Chrome", "Back", "chevron-left"),
    ("caret-up", "Chrome", "Collapse up", "chevron-up"),
    ("arrow-left", "Chrome", "Go back", "arrow-left"),
    ("arrow-right", "Chrome", "Go forward", "arrow-right"),
    ("arrow-up", "Chrome", "Scroll up", "arrow-up"),
    ("arrow-down", "Chrome", "Jump to present", "arrow-down"),
    ("arrow-circle-up", "Chrome", "Boost up", "arrow-up-circle"),
    ("arrow-line-up", "Chrome", "Move to top", "arrow-up5"),
    ("arrow-line-down", "Chrome", "Move to bottom", "arrow-down5"),
    ("arrow-line-right", "Chrome", "Move to end", "arrow-right5"),
    ("arrow-square-out", "Chrome", "Open externally", "arrow-up-right-square"),
    ("arrow-square-in", "Chrome", "Open in app", "arrow-down-left-square"),
    ("arrows-in-line-vertical", "Chrome", "Collapse all", "CUSTOM:collapse-all"),
    ("arrows-down-up", "Chrome", "Sort", "sort"),
    ("arrows-left-right", "Chrome", "Swap", "arrow-swap-horizontal"),
    ("arrows-out-cardinal", "Chrome", "Move", "maximize"),
    ("arrows-out-line-horizontal", "Chrome", "Expand width", "slider-h"),
    ("arrow-clockwise", "Chrome", "Retry", "refresh2"),
    ("arrow-counter-clockwise", "Chrome", "Reset", "rotate-left"),
    ("arrows-clockwise", "Chrome", "Sync", "refresh"),
    ("clock-counter-clockwise", "Chrome", "Recent", "history"),
    ("x", "Chrome", "Close", "xmark"),
    ("plus", "Chrome", "Add", "add"),
    ("minus", "Chrome", "Remove", "minus"),
    ("check", "Chrome", "Done", "check3"),
    ("magnifying-glass", "Chrome", "Search", "search-normal"),
    ("magnifying-glass-plus", "Chrome", "Zoom in", "search-zoom-in"),
    ("magnifying-glass-minus", "Chrome", "Zoom out", "search-zoom-out"),
    ("dots-three", "Chrome", "More", "more-h"),
    ("dots-three-vertical", "Chrome", "More (vertical)", "more"),
    ("dots-six-vertical", "Chrome", "Drag handle", "CUSTOM:grip"),
    ("dots-six", "Chrome", "Drag handle (horizontal)", "CUSTOM:grip"),
    ("funnel", "Chrome", "Filter", "filter"),
    ("funnel-simple", "Chrome", "Filter (simple)", "filter3"),
    ("sort-descending", "Chrome", "Sort newest", "sort-desc"),
    ("sliders-horizontal", "Chrome", "Adjust", "tuning2"),
    ("rows", "Chrome", "Compact list", "row-vertical"),
    ("squares-four", "Chrome", "Grid", "category"),
    ("stack", "Chrome", "Stack", "layers"),
    ("swap", "Chrome", "Swap device", "convert"),
    ("circle", "Chrome", "Radio off", "CUSTOM:dot-ring"),
    ("circle-dashed", "Chrome", "Placeholder", "CUSTOM:dash-ring"),
    ("square", "Window", "Maximize", "CUSTOM:win-max"),
    ("bell", "Status", "Notifications", "bell"),
    ("bell-ringing", "Status", "All messages", "bell-ring"),
    ("bell-slash", "Status", "Muted channel", "bell-off2"),
    ("info", "Status", "Info", "info-circle"),
    ("question", "Status", "Help", "help-circle"),
    ("warning", "Status", "Warning", "triangle-warning"),
    ("warning-circle", "Status", "Error", "alert-circle"),
    ("check-circle", "Status", "Success", "check-circle"),
    ("x-circle", "Status", "Failed", "x-circle"),
    ("prohibit", "Status", "Blocked", "forbidden"),
    ("eye", "Status", "Show", "eye"),
    ("eye-slash", "Status", "Hide", "eye-slash"),
    ("sparkle", "Status", "New", "sparkle"),
    ("star", "Status", "Featured", "star"),
    ("lightbulb", "Status", "Tip", "lightbulb"),
    ("trophy", "Status", "Achievement", "cup-trophy"),
    ("infinity", "Status", "Never expires", "infinite"),
    ("gauge", "Status", "Performance", "speedometer"),
    ("trend-up", "Status", "Growing", "trend-up"),
    ("chart-bar", "Status", "Insights", "chart-bar"),
    ("chart-bar-horizontal", "Status", "Poll", "poll"),
    ("cell-signal-full", "Status", "Connection good", "CUSTOM:signal-3"),
    ("cell-signal-medium", "Status", "Connection fair", "CUSTOM:signal-2"),
    ("cell-signal-low", "Status", "Connection poor", "CUSTOM:signal-1"),
    ("cell-signal-x", "Status", "No connection", "CUSTOM:signal-x"),
    ("wifi-slash", "Status", "Offline", "wifi-off"),
    ("cloud-slash", "Status", "Not synced", "cloud-cross"),
    ("cloud-x", "Status", "Sync failed", "cloud-x"),
    ("moon", "Status", "Idle", "moon"),
    ("moon-stars", "Status", "Do not disturb (night)", "moon-stars"),
    ("sun", "Status", "Light theme", "sun"),
    ("drop-half", "Status", "Contrast", "dark-light"),
    ("calendar-blank", "Status", "Event", "calendar"),
    ("calendar-check", "Status", "Event joined", "calendar-tick"),
    ("calendar-plus", "Status", "New event", "calendar-add"),
    ("calendar-x", "Status", "Event cancelled", "calendar-x"),
    ("map-pin", "Status", "Location", "map-point"),
    ("globe-simple", "Status", "Language", "globe"),
    ("globe-hemisphere-west", "Status", "Region", "earth"),
    ("image", "Media", "Image", "image"),
    ("image-square", "Media", "Thumbnail", "gallery-wide"),
    ("image-broken", "Media", "Image failed", "gallery-slash"),
    ("file", "Media", "File", "file"),
    ("file-image", "Media", "Image file", "gallery"),
    ("file-audio", "Media", "Audio file", "music-square"),
    ("file-video", "Media", "Video file", "video-play"),
    ("file-pdf", "Media", "PDF", "file-pdf"),
    ("file-zip", "Media", "Archive file", "file-zip"),
    ("file-xls", "Media", "Spreadsheet", "file-text"),
    ("file-x", "Media", "File failed", "file-remove"),
    ("film-strip", "Media", "Video", "film"),
    ("upload-simple", "Media", "Upload", "upload"),
    ("download-simple", "Media", "Download", "download"),
    ("floppy-disk", "Media", "Save", "floppy"),
    ("archive", "Media", "Archive", "archive"),
    ("gear-six", "Settings", "Settings", "setting"),
    ("palette", "Settings", "Appearance", "palette"),
    ("keyboard", "Settings", "Keybinds", "keyboard"),
    ("key", "Settings", "Password", "key"),
    ("password", "Settings", "Passcode", "password"),
    ("fingerprint", "Settings", "Biometric", "fingerprint"),
    ("lock-key-open", "Settings", "Unlocked", "lock-keyhole-open"),
    ("sign-in", "Settings", "Sign in", "login"),
    ("sign-out", "Settings", "Sign out", "logout"),
    ("device-mobile", "Settings", "Phone", "mobile"),
    ("device-mobile-slash", "Settings", "Phone removed", "mobile-off"),
    ("laptop", "Settings", "Laptop", "laptop"),
    ("desktop", "Settings", "Desktop", "desktop"),
    ("devices", "Settings", "Devices", "devices"),
    ("browser", "Settings", "Browser", "browser-code"),
    ("terminal-window", "Settings", "Developer", "browser-terminal"),
    ("command", "Settings", "Shortcut", "command"),
    ("bug", "Settings", "Report a bug", "bug"),
    ("robot", "Settings", "Bot", "CUSTOM:bot"),
    ("webhooks-logo", "Settings", "Webhook", "nodes"),
    ("plugs", "Settings", "Integration", "plug"),
    ("plugs-connected", "Settings", "Connected", "plug2"),
    ("git-branch", "Settings", "Version", "branch-down"),
    ("share-network", "Settings", "Share", "share"),
    ("share-fat", "Settings", "Share (fat)", "share3"),
    ("toggle-right", "Settings", "Toggle", "toggle-on"),
    ("eyedropper", "Settings", "Pick colour", "dropper"),
    ("first-aid", "Settings", "Troubleshoot", "medical-kit"),
    ("leaf", "Emoji", "Nature", "leaf"),
    ("hamburger", "Emoji", "Food", "pizza-slice"),
    ("soccer-ball", "Emoji", "Activity", "football"),
    ("airplane-tilt", "Emoji", "Travel", "airplane"),
    ("plant", "Emoji", "Plant", "tree"),
    ("sword", "Emoji", "Gaming role", "GAP"),
    ("apple-logo", "Brand", "Apple", "BRAND"),
    ("github-logo", "Brand", "GitHub", "BRAND"),
    ("spotify-logo", "Brand", "Spotify", "BRAND"),
    ("steam-logo", "Brand", "Steam", "BRAND"),
    ("twitch-logo", "Brand", "Twitch", "BRAND"),
    ("Nav::Minimize", "Window", "Minimize", "minus"),
    ("Nav::Restore", "Window", "Restore", "CUSTOM:win-restore"),
]


def thicken(body):
    if 'stroke-width="1.5"' in body:
        return body.replace('stroke-width="1.5"', 'stroke-width="2"')
    return f'<g stroke="currentColor" stroke-width="0.5" stroke-linejoin="round" paint-order="stroke fill">{body}</g>'


def usage():
    base = lambda n: re.sub(r"-(bold|fill)$", "", n)
    src = {p: p.read_text() for p in (ROOT / "src").rglob("*.rs")}
    uses = lambda pat: sorted({str(p.relative_to(ROOT / "src"))[:-3] for p, t in src.items() if re.search(pat, t)})
    inv = {}
    icons = (ROOT / "src/icons.rs").read_text()
    phos = re.findall(r'phosphor/([a-z0-9-]+)\.svg', icons.split("const BRAND")[0])
    variants = re.findall(r"^\s+([A-Z]\w+),$", icons.split("pub enum Glyph")[1].split("}")[0], re.M)
    pairs = [(f"Glyph::{v}", p) for v, p in zip(variants, phos)]
    pairs += [(f"Nav::{v}", p) for v, p in re.findall(r'Nav::(\w+) => include_bytes!\("\.\./assets/icons/phosphor/([a-z0-9-]+)\.svg', src[ROOT / "src/sidebar.rs"])]
    for var, p in pairs:
        inv.setdefault(base(p), {"rust": {}, "spec": Counter()})["rust"][var] = uses(rf"{var}\b")
    for f in [*(ROOT / "design").glob("*.html"), ROOT / "design/site.js", *(ROOT / "design/css").glob("*.css")]:
        if f.name == "icons.html":
            continue
        for n in re.findall(r"\bph-([a-z0-9-]+)", f.read_text()):
            if n not in ("bold", "fill"):
                inv.setdefault(n, {"rust": {}, "spec": Counter()})["spec"][f.stem] += 1
    return inv


def main():
    reicon = json.loads(Path(sys.argv[1]).read_text())["icons"]
    inv = usage()
    rows, need = [], set()
    # Rust variants named in the map leave the row their Phosphor file would put them in.
    pulled = {k: next(v["rust"].pop(k) for v in inv.values() if k in v["rust"]) for k, *_ in MAP if "::" in k}
    for key, group, meaning, target in MAP:
        if "::" in key:
            u = {"rust": {key: pulled[key]}, "spec": {}}
            src = None
        else:
            u = inv.pop(key, {"rust": {}, "spec": {}})
            src = key
        if not u["rust"] and not u["spec"]:
            continue
        if not target.startswith(("CUSTOM", "BRAND", "GAP")):
            need.add(target)
        rows.append({"was": src, "group": group, "meaning": meaning, "to": target, "rust": u["rust"], "spec": dict(u["spec"])})
    left = [k for k, v in inv.items() if v["rust"] or v["spec"]]
    if left:
        sys.exit(f"unmapped: {left}")
    missing = [n for n in need if n not in reicon]
    if missing:
        sys.exit(f"not in Reicon: {missing}")
    bodies = {n: thicken(reicon[n]["body"]) for n in sorted(need | EXTRA)}
    out = ROOT / "design/icons-data.js"
    out.write_text(
        "// Generated by tools/reicon.py; edit the map there.\n"
        f"const REICON = {json.dumps(bodies, separators=(',', ':'))};\n"
        f"const ICON_MAP = {json.dumps(rows, separators=(',', ':'))};\n"
    )
    print(f"{len(rows)} rows, {len(bodies)} Reicon glyphs -> {out.relative_to(ROOT)}")


# Bases the custom glyphs in design/icons.html are built on.
EXTRA = {"hashtag2", "volume-high", "bullhorn", "podcast", "messages2", "gallery", "messages3", "microphone2", "headphone", "video", "screencast", "monitor", "sparkle"}

if __name__ == "__main__":
    main()
