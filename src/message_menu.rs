// Discord's message context menu (navId "message"), item-for-item from its menu code
// (docs/discord-menus-user-message.md §5). Items for features Rustcord doesn't have yet (forward,
// threads, translate, bookmarks, reminders, apps, reports) are left out.
use gpui_kit::{Context, Pixels, Point};
use reqwest::Method;

use crate::{
    EmojiTarget, Rustcord,
    discord::{Emoji, Message, SendState, perm},
    icons::Glyph,
    menu::{Entry, item, submenu},
};

// Discord's padding for the frequent-emoji rows when there's no history yet.
pub const QUICK_PAD: [&str; 12] = ["👍", "❤️", "😂", "💯", "😆", "💖", "🔥", "😭", "🙏", "👀", "😮", "🎉"];

impl Rustcord {
    pub(crate) fn open_message_menu(&mut self, message: Message, at: Point<Pixels>, cx: &mut Context<Self>) {
        self.open_live_menu(at, move |this| this.message_menu(&message), cx);
    }

    // In a server, whether we may moderate messages here (server-wide permission; channel overwrites aside).
    fn can_manage_messages(&self, message: &Message) -> bool {
        let (Some(me), Some(guild)) = (self.me.as_ref(), message.guild_id.as_ref().and_then(|g| self.guilds.iter().find(|x| &x.id == g))) else { return false };
        guild.can(&me.id, perm::MANAGE_MESSAGES)
    }

    pub(crate) fn message_menu(&self, message: &Message) -> Vec<Entry> {
        if message.state == SendState::Failed {
            let (retry, delete) = (message.clone(), message.clone());
            return vec![
                item("Resend message", move |this, _, cx| this.retry(retry.clone(), cx)).entry(),
                item("Delete Message", move |this, _, cx| this.delete(delete.clone(), cx)).danger().entry(),
            ];
        }
        if message.state != SendState::Sent {
            return Vec::new();
        }
        let mine = self.me.as_ref().is_some_and(|me| me.id == message.author.id);
        let in_dm = message.guild_id.is_none();
        let manage = self.can_manage_messages(message);
        let mut out = Vec::new();

        // G3: reactions.
        let react = |emoji: Emoji, m: &Message| {
            let m = m.clone();
            move |this: &mut Rustcord, _: &mut gpui_kit::Window, cx: &mut Context<Rustcord>| this.toggle_reaction(&m, emoji.clone(), cx)
        };
        let quick: Vec<_> = self.prefs.frequent_emojis(4, &QUICK_PAD).into_iter().map(|e| (e.clone(), std::rc::Rc::new(react(e, message)) as crate::menu::Act)).collect();
        out.push(Entry::Emojis(quick));
        let mut more: Vec<Entry> = self.prefs.frequent_emojis(12, &QUICK_PAD).into_iter().map(|e| item(e.name.clone(), react(e, message)).entry()).collect();
        more.push(Entry::Separator);
        let picker = message.clone();
        more.push(item("View More", move |this, window, cx| this.open_emoji_picker(EmojiTarget::Reaction(Box::new(picker.clone())), window, cx)).entry());
        out.push(submenu("Add Reaction", more).entry());
        out.push(Entry::Separator);

        // G4: primary.
        if mine {
            let m = message.clone();
            out.push(item("Edit Message", move |this, window, cx| this.start_edit(m.clone(), window, cx)).icon(Glyph::Pen).entry());
        }
        let m = message.clone();
        out.push(item("Reply", move |this, window, cx| this.start_reply(m.clone(), window, cx)).icon(Glyph::Reply).entry());
        let in_text = self.guilds.iter().flat_map(|g| g.channels.iter()).any(|c| c.id == message.channel_id && c.kind == crate::discord::ChannelKind::Text);
        let can_thread = message.guild_id.as_ref().and_then(|g| self.guilds.iter().find(|x| &x.id == g)).zip(self.me.as_ref()).is_some_and(|(g, me)| g.can(&me.id, perm::CREATE_PUBLIC_THREADS));
        if in_text && can_thread && message.thread.is_none() {
            let m = message.clone();
            out.push(item("Create Thread", move |this, window, cx| this.create_thread(m.clone(), window, cx)).icon(Glyph::Thread).entry());
        }
        out.push(Entry::Separator);

        // G5: secondary.
        if !message.content.is_empty() {
            let text = message.content.clone();
            out.push(item("Copy Text", move |this, _, cx| this.copy_text(text.clone(), cx)).icon(Glyph::Copy).entry());
        }
        if in_dm || manage {
            let m = message.clone();
            let (label, method) = if message.pinned { ("Unpin Message", Method::DELETE) } else { ("Pin Message", Method::PUT) };
            out.push(
                item(label, move |this, _, cx| {
                    let path = format!("/channels/{}/messages/pins/{}", m.channel_id, m.id);
                    let (channel, id, pinned) = (m.channel_id.clone(), m.id.clone(), method == Method::PUT);
                    this.rest(method.clone(), path, None, move |this, _| {
                        if let Some(m) = this.message_mut(&channel, &id) {
                            m.pinned = pinned;
                        }
                    }, cx);
                })
                .icon(Glyph::Pin)
                .entry(),
            );
        }
        let m = message.clone();
        out.push(item("Mark Unread", move |this, _, cx| this.mark_unread(&m, cx)).entry());
        let m = message.clone();
        out.push(item("Copy Message Link", move |this, _, cx| {
            let link = this.message_link(&m);
            this.copy_text(link, cx)
        }).entry());
        if cfg!(target_os = "macos") && !message.content.is_empty() {
            let text = message.content.clone();
            out.push(item("Speak Message", move |_, _, _| speak(&text)).entry());
        }
        out.push(Entry::Separator);

        // G6: destructive.
        if manage && !message.reactions.is_empty() {
            let per_emoji: Vec<Entry> = message
                .reactions
                .iter()
                .map(|r| {
                    let (m, emoji) = (message.clone(), r.emoji.clone());
                    item(r.emoji.name.clone(), move |this, _, cx| {
                        let path = format!("/channels/{}/messages/{}/reactions/{}", m.channel_id, m.id, emoji.path());
                        this.rest(Method::DELETE, path, None, |_, _| {}, cx);
                    })
                    .entry()
                })
                .collect();
            out.push(submenu("Remove Reactions", per_emoji).danger().entry());
            let m = message.clone();
            out.push(
                item("Remove All Reactions", move |this, _, cx| {
                    let m = m.clone();
                    this.confirm("Remove All Reactions", "Are you sure you want to remove all the reactions on this message?", "Remove All", move |this, _, cx| {
                        this.rest(Method::DELETE, format!("/channels/{}/messages/{}/reactions", m.channel_id, m.id), None, |_, _| {}, cx);
                    }, cx);
                })
                .danger()
                .entry(),
            );
        }
        if mine || manage {
            let m = message.clone();
            out.push(
                item("Delete Message", move |this, _, cx| {
                    let m = m.clone();
                    this.confirm("Delete Message", "Are you sure you want to delete this message?", "Delete", move |this, _, cx| this.delete(m.clone(), cx), cx);
                })
                .icon(Glyph::Trash)
                .danger()
                .entry(),
            );
        }
        out.push(Entry::Separator);

        // G9: developer.
        if self.developer_mode {
            let id = message.id.clone();
            out.push(item("Copy Message ID", move |this, _, cx| this.copy_text(id.clone(), cx)).icon(Glyph::Id).entry());
        }
        out
    }
}

// Discord's "Speak Message", through the system voice.
fn speak(text: &str) {
    let text = text.to_owned();
    // Through stdin, so text starting with "-" can't be read as `say` options.
    std::thread::spawn(move || {
        use std::io::Write;
        let Ok(mut child) = std::process::Command::new("say").stdin(std::process::Stdio::piped()).spawn() else { return };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        let _ = child.wait();
    });
}
