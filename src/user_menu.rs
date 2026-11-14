// Discord's user context menus, built item-for-item from its own menu code (docs/discord-menus-user-
// message.md, read from Discord web 2026-11-14). Each item carries Discord's show-condition; items
// for features Rustcord doesn't have (activities, apps, stages, mod view, E2EE codes) are left out.
use gpui_kit::{Context, Pixels, Point, Window};
use reqwest::Method;
use serde_json::json;

use crate::{
    Rustcord,
    discord::{ChannelKind, Relation, User, perm},
    icons::Glyph,
    menu::{Entry, item, slider, submenu},
};

// Where the menu was opened, which decides Discord's variant and its flags.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    // Server chat: author name or avatar, member list (`chat`).
    ServerChat,
    // Server voice row or call tile (`media`).
    ServerVoice,
    // A 1:1 DM: the DM list row or a DM author.
    Dm,
    // No channel: the Friends list.
    Generic,
}

// Discord's timeout lengths, in seconds.
const TIMEOUTS: [(&str, i64); 6] = [("60 Seconds", 60), ("5 Minutes", 300), ("10 Minutes", 600), ("1 Hour", 3600), ("1 Day", 86_400), ("1 Week", 604_800)];

impl Rustcord {
    pub(crate) fn open_user_menu(&mut self, user: User, guild_id: Option<String>, surface: Surface, at: Point<Pixels>, cx: &mut Context<Self>) {
        let entries = self.user_menu(&user, guild_id.as_deref(), surface, at);
        self.open_menu(at, entries, cx);
    }

    // Discord's DM row menu: read state, the DM user's items, then Close DM and the channel's ID.
    pub(crate) fn open_dm_menu(&mut self, channel: crate::discord::Channel, at: Point<Pixels>, cx: &mut Context<Self>) {
        let mut entries = Vec::new();
        let unread = self.unread(&channel.id);
        let id = channel.id.clone();
        let read = item("Mark As Read", move |this, _, cx| this.mark_channel_read(&id, cx));
        entries.push(if unread { read } else { read.disabled() }.entry());
        entries.push(Entry::Separator);
        if let Some(user) = channel.recipient.clone() {
            entries.extend(self.user_menu(&user, None, Surface::Dm, at).into_iter().filter(|e| !e.is_copy_id()));
        }
        let id = channel.id.clone();
        entries.push(item("Close DM", move |this, _, cx| this.close_dm(&id, cx)).entry());
        entries.push(Entry::Separator);
        if self.developer_mode {
            if let Some(user) = &channel.recipient {
                let uid = user.id.clone();
                entries.push(item("Copy User ID", move |this, _, cx| this.copy_text(uid.clone(), cx)).icon(Glyph::Id).entry());
            }
            let cid = channel.id.clone();
            entries.push(item("Copy Channel ID", move |this, _, cx| this.copy_text(cid.clone(), cx)).icon(Glyph::Id).entry());
        }
        self.open_menu(at, entries, cx);
    }

    // Closes a DM (Discord's DELETE on the channel only hides it from the list for us).
    fn close_dm(&mut self, channel_id: &str, cx: &mut Context<Self>) {
        let id = channel_id.to_owned();
        self.dms.retain(|d| d.id != id);
        if self.channel.as_ref().is_some_and(|c| c.id == id) {
            self.open_home(cx);
        }
        self.rest(Method::DELETE, format!("/channels/{id}"), None, |_, _| {}, cx);
    }

    // An author's name or avatar in chat: the server menu in a server, the DM menu in a DM.
    pub(crate) fn open_author_menu(&mut self, user: User, at: Point<Pixels>, cx: &mut Context<Self>) {
        let surface = if self.guild_id.is_some() { Surface::ServerChat } else { Surface::Dm };
        let guild_id = self.guild_id.clone();
        self.open_user_menu(user, guild_id, surface, at, cx);
    }

    fn relation(&self, user_id: &str) -> Option<Relation> {
        self.relationships.iter().find(|r| r.user.id == user_id).map(|r| r.kind.clone())
    }

    // Discord's canManageUser: the permission, and my highest role above theirs (or I own the server).
    fn can_manage(&self, guild_id: &str, target: &str, bit: u64) -> bool {
        let (Some(guild), Some(me)) = (self.guilds.iter().find(|g| g.id == guild_id), self.me.as_ref()) else { return false };
        if !guild.can(&me.id, bit) || guild.owner_id.as_deref() == Some(target) {
            return false;
        }
        let theirs = self.member_roles.get(guild_id).and_then(|r| r.get(target)).map(|roles| guild.roles.iter().filter(|r| roles.contains(&r.id)).map(|r| r.position).max().unwrap_or(0)).unwrap_or(0);
        guild.top_position(&me.id) > theirs
    }

    fn user_menu(&self, user: &User, guild_id: Option<&str>, surface: Surface, at: Point<Pixels>) -> Vec<Entry> {
        let Some(me) = self.me.clone() else { return Vec::new() };
        let target = user.id.clone();
        let is_me = target == me.id;
        let media = surface == Surface::ServerVoice;
        let in_server = matches!(surface, Surface::ServerChat | Surface::ServerVoice);
        let guild = guild_id.filter(|_| in_server).and_then(|id| self.guilds.iter().find(|g| g.id == id));
        let relation = self.relation(&target);
        let (friend, blocked) = (relation == Some(Relation::Friend), relation == Some(Relation::Blocked));
        let ignored = self.ignored.contains(&target);
        let name = user.name.clone();
        let mut out: Vec<Entry> = Vec::new();

        // G2: profile and social.
        let profile_user = user.clone();
        out.push(item("Profile", move |this, _, cx| this.open_profile(profile_user.clone(), at, cx)).entry());
        if surface == Surface::ServerChat && self.channel.is_some() {
            let mention = user.clone();
            out.push(item("Mention", move |this, window, cx| this.mention_user(&mention, window, cx)).entry());
        }
        if !is_me && surface != Surface::Dm {
            let id = target.clone();
            out.push(item("Message", move |this, window, cx| this.message_user(id.clone(), window, cx)).entry());
        }
        let in_dm_call = self.call.as_ref().is_some_and(|c| c.channel.kind == ChannelKind::Dm && c.channel.recipient.as_ref().is_some_and(|r| r.id == target));
        if !is_me && !user.bot && !in_dm_call {
            let id = target.clone();
            let start = item("Start a Call", move |this, window, cx| this.start_call(id.clone(), window, cx));
            out.push(if blocked { start.disabled() } else { start }.entry());
        }
        if !is_me {
            let (id, who) = (target.clone(), name.clone());
            out.push(item("Add Note", move |this, window, cx| this.edit_note(id.clone(), who.clone(), window, cx)).hint("Only visible to you").entry());
        }
        if friend {
            let (id, who) = (target.clone(), name.clone());
            out.push(item("Add Friend Nickname", move |this, window, cx| this.edit_friend_nickname(id.clone(), who.clone(), window, cx)).entry());
        }
        out.push(Entry::Separator);

        // G3: volume.
        if media && !is_me {
            let volume = self.prefs.user_volumes.get(&target).copied().unwrap_or(1.);
            let id = target.clone();
            out.push(slider("User Volume", format!("user-volume-{target}"), volume / 2., move |this, f, cx| this.set_user_volume(&id, f * 2., cx)));
            out.push(Entry::Separator);
        }

        // G5: media and social actions.
        if media && is_me {
            out.push(item("Mute", |this, _, cx| this.toggle_mute(cx)).checked(self.muted || self.deafened).entry());
            out.push(item("Deafen", |this, _, cx| this.toggle_deafen(cx)).checked(self.deafened).entry());
        }
        if media && !is_me {
            let id = target.clone();
            out.push(item("Mute", move |this, _, cx| this.toggle_local_mute(&id, cx)).checked(self.prefs.local_mutes.contains(&target)).entry());
            let id = target.clone();
            out.push(
                item("Mute Soundboard", move |this, _, cx| this.update_prefs(cx, |p| toggle(&mut p.soundboard_mutes, &id))).checked(self.prefs.soundboard_mutes.contains(&target)).entry(),
            );
            let id = target.clone();
            out.push(item("Disable Video", move |this, _, cx| this.update_prefs(cx, |p| toggle(&mut p.hidden_video, &id))).checked(self.prefs.hidden_video.contains(&target)).entry());
        }
        if let Some(guild) = guild {
            let gid = guild.id.clone();
            if is_me {
                let current = self.members.get(&gid).and_then(|m| m.get(&me.id)).map(|u| u.name.clone()).unwrap_or_default();
                out.push(item("Edit Per-server Profile", move |this, window, cx| this.edit_nickname(gid.clone(), "@me".into(), current.clone(), window, cx)).entry());
            } else if self.can_manage(&guild.id, &target, perm::MANAGE_NICKNAMES) {
                let (id, current) = (target.clone(), name.clone());
                out.push(item("Change Nickname", move |this, window, cx| this.edit_nickname(gid.clone(), id.clone(), current.clone(), window, cx)).entry());
            }
        }
        if !is_me && !user.bot && !blocked && !ignored {
            let invites: Vec<Entry> = self
                .guilds
                .iter()
                .filter(|g| Some(g.id.as_str()) != guild_id && g.can(&me.id, perm::CREATE_INSTANT_INVITE))
                .map(|g| {
                    let (gid, uid) = (g.id.clone(), target.clone());
                    item(g.name.clone(), move |this, window, cx| this.invite_to_server(gid.clone(), uid.clone(), window, cx)).entry()
                })
                .collect();
            if !invites.is_empty() {
                out.push(submenu("Invite to Server", invites).entry());
            }
        }
        if !is_me && !user.bot {
            let id = target.clone();
            let who = name.clone();
            out.push(match relation {
                Some(Relation::Friend) => item("Remove Friend", move |this, _, cx| {
                    let id = id.clone();
                    this.confirm(format!("Remove '{who}'"), format!("Are you sure you want to remove {who} from your friends?"), "Remove Friend", move |this, _, cx| {
                        this.rest(Method::DELETE, format!("/users/@me/relationships/{id}"), None, |_, _| {}, cx);
                    }, cx);
                })
                .entry(),
                Some(Relation::Incoming) => item("Accept Friend Request", move |this, _, cx| this.rest(Method::PUT, format!("/users/@me/relationships/{id}"), Some(json!({})), |_, _| {}, cx)).entry(),
                Some(Relation::Outgoing) => item("Friend Request Sent", |_, _, _| {}).disabled().entry(),
                Some(Relation::Blocked) => item("Add Friend", |_, _, _| {}).disabled().entry(),
                None => item("Add Friend", move |this, _, cx| this.rest(Method::PUT, format!("/users/@me/relationships/{id}"), Some(json!({})), |_, _| {}, cx)).entry(),
            });
        }
        if !is_me && !blocked {
            let (id, who) = (target.clone(), name.clone());
            out.push(match ignored {
                true => item("Unignore", move |this, _, cx| this.set_ignored(id.clone(), false, cx)).entry(),
                false => item("Ignore", move |this, _, cx| {
                    let id = id.clone();
                    this.confirm(format!("Ignore {who}?"), "You won't see their messages or be notified by them, and they won't know.", "Ignore", move |this, _, cx| this.set_ignored(id.clone(), true, cx), cx);
                })
                .entry(),
            });
        }
        if !is_me {
            let (id, who) = (target.clone(), name.clone());
            out.push(match blocked {
                true => item("Unblock", move |this, _, cx| this.rest(Method::DELETE, format!("/users/@me/relationships/{id}"), None, |this, cx| this.show_toast("User unblocked", cx), cx)).entry(),
                false => item("Block", move |this, _, cx| {
                    let id = id.clone();
                    this.confirm(format!("Block {who}?"), "They won't be able to message you, and their messages will be hidden.", "Block", move |this, _, cx| {
                        this.rest(Method::PUT, format!("/users/@me/relationships/{id}"), Some(json!({ "type": 2 })), |_, _| {}, cx);
                    }, cx);
                })
                .danger()
                .entry(),
            });
        }
        out.push(Entry::Separator);

        // G6: roles and voice moves.
        let voice = guild.and_then(|g| g.voice_states.iter().find(|v| v.user_id == target).map(|v| (g, v.clone())));
        if let Some(guild) = guild {
            let theirs = self.member_roles.get(&guild.id).and_then(|r| r.get(&target)).cloned().unwrap_or_default();
            let manage = guild.can(&me.id, perm::MANAGE_ROLES);
            let top = guild.top_position(&me.id);
            let mut roles: Vec<_> = guild.roles.iter().filter(|r| r.id != guild.id && !r.managed).collect();
            roles.sort_by_key(|r| std::cmp::Reverse(r.position));
            let rows: Vec<Entry> = roles
                .into_iter()
                .filter(|r| theirs.contains(&r.id) || (manage && r.position < top))
                .map(|role| {
                    let has = theirs.contains(&role.id);
                    if !manage {
                        return item(role.name.clone(), |_, _, _| {}).entry();
                    }
                    let (gid, uid, rid) = (guild.id.clone(), target.clone(), role.id.clone());
                    let row = item(role.name.clone(), move |this, _, cx| {
                        let method = if has { Method::DELETE } else { Method::PUT };
                        this.rest(method, format!("/guilds/{gid}/members/{uid}/roles/{rid}"), None, |_, _| {}, cx);
                    })
                    .checked(has);
                    if role.position >= top { row.disabled() } else { row }.entry()
                })
                .collect();
            if !rows.is_empty() {
                let label = if rows.len() == 1 { "Role" } else { "Roles" };
                out.push(submenu(label, rows).entry());
            }
        }
        if let Some((guild, state)) = &voice {
            let can_move = is_me || guild.can(&me.id, perm::MOVE_MEMBERS);
            let channels: Vec<Entry> = guild
                .channels
                .iter()
                .filter(|c| c.kind == ChannelKind::Voice && state.channel_id.as_ref() != Some(&c.id))
                .map(|c| {
                    let (gid, uid, channel) = (guild.id.clone(), target.clone(), c.clone());
                    item(c.name.clone(), move |this, _, cx| {
                        match is_me {
                            true => this.join_voice(channel.clone(), cx),
                            false => this.rest(Method::PATCH, format!("/guilds/{gid}/members/{uid}"), Some(json!({ "channel_id": channel.id })), |_, _| {}, cx),
                        }
                    })
                    .icon(Glyph::VolumeHigh)
                    .entry()
                })
                .collect();
            if can_move && !channels.is_empty() {
                out.push(submenu("Move to", channels).entry());
            }
        }
        out.push(Entry::Separator);

        // G8: moderation.
        if let Some((guild, state)) = &voice {
            let gid = guild.id.clone();
            if guild.can(&me.id, perm::MUTE_MEMBERS) {
                let (gid, uid, on) = (gid.clone(), target.clone(), state.server_muted);
                out.push(item("Server Mute", move |this, _, cx| this.rest(Method::PATCH, format!("/guilds/{gid}/members/{uid}"), Some(json!({ "mute": !on })), |_, _| {}, cx)).checked(on).danger().entry());
            }
            if guild.can(&me.id, perm::DEAFEN_MEMBERS) {
                let (gid, uid, on) = (gid.clone(), target.clone(), state.server_deafened);
                out.push(item("Server Deafen", move |this, _, cx| this.rest(Method::PATCH, format!("/guilds/{gid}/members/{uid}"), Some(json!({ "deaf": !on })), |_, _| {}, cx)).checked(on).danger().entry());
            }
            if !is_me && guild.can(&me.id, perm::MOVE_MEMBERS) {
                let (gid, uid) = (gid.clone(), target.clone());
                out.push(item("Disconnect", move |this, _, cx| this.rest(Method::PATCH, format!("/guilds/{gid}/members/{uid}"), Some(json!({ "channel_id": null })), |_, _| {}, cx)).danger().entry());
            }
        }
        if let Some(guild) = guild.filter(|_| !is_me) {
            let theirs = self.member_roles.get(&guild.id).and_then(|r| r.get(&target)).cloned().unwrap_or_default();
            let admin = guild.roles.iter().any(|r| theirs.contains(&r.id) && r.permissions & perm::ADMINISTRATOR != 0);
            if !admin && self.can_manage(&guild.id, &target, perm::MODERATE_MEMBERS) {
                let lengths: Vec<Entry> = TIMEOUTS
                    .iter()
                    .map(|&(label, seconds)| {
                        let (gid, uid) = (guild.id.clone(), target.clone());
                        item(label, move |this, _, cx| {
                            let until = (chrono::Utc::now() + chrono::Duration::seconds(seconds)).to_rfc3339();
                            this.rest(Method::PATCH, format!("/guilds/{gid}/members/{uid}"), Some(json!({ "communication_disabled_until": until })), |_, _| {}, cx);
                        })
                        .entry()
                    })
                    .collect();
                out.push(submenu(format!("Timeout {name}"), lengths).danger().entry());
            }
            if self.can_manage(&guild.id, &target, perm::KICK_MEMBERS) {
                let (gid, uid, who) = (guild.id.clone(), target.clone(), name.clone());
                out.push(
                    item(format!("Kick {name}"), move |this, _, cx| {
                        let (gid, uid) = (gid.clone(), uid.clone());
                        this.confirm(format!("Kick '{who}' from Server"), format!("Are you sure you want to kick {who} from the server? They will be able to rejoin again with a new invite."), "Kick", move |this, _, cx| {
                            this.rest(Method::DELETE, format!("/guilds/{gid}/members/{uid}"), None, |_, _| {}, cx);
                        }, cx);
                    })
                    .danger()
                    .entry(),
                );
            }
            if !user.bot && self.can_manage(&guild.id, &target, perm::BAN_MEMBERS) {
                let (gid, uid, who) = (guild.id.clone(), target.clone(), name.clone());
                out.push(
                    item(format!("Ban {name}"), move |this, _, cx| {
                        let (gid, uid) = (gid.clone(), uid.clone());
                        this.confirm(format!("Ban '{who}'"), format!("Are you sure you want to ban {who}? They won't be able to rejoin unless unbanned."), "Ban", move |this, _, cx| {
                            this.rest(Method::PUT, format!("/guilds/{gid}/bans/{uid}"), Some(json!({ "delete_message_seconds": 0 })), |_, _| {}, cx);
                        }, cx);
                    })
                    .danger()
                    .entry(),
                );
            }
        }
        out.push(Entry::Separator);

        // G9: developer.
        if self.developer_mode {
            let id = target.clone();
            out.push(item("Copy User ID", move |this, _, cx| this.copy_text(id.clone(), cx)).icon(Glyph::Id).entry());
        }
        out
    }

    fn edit_note(&mut self, user_id: String, name: String, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt(format!("Note for {name}"), "Only visible to you.", "", "Save", move |this, note, _, cx| {
            this.rest(Method::PUT, format!("/users/@me/notes/{user_id}"), Some(json!({ "note": note })), |_, _| {}, cx);
        }, window, cx);
    }

    fn edit_friend_nickname(&mut self, user_id: String, name: String, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt("Add Friend Nickname", format!("A name only you see for {name}."), "", "Save", move |this, nick, _, cx| {
            let nickname = (!nick.trim().is_empty()).then(|| nick.trim().to_owned());
            this.rest(Method::PATCH, format!("/users/@me/relationships/{user_id}"), Some(json!({ "nickname": nickname })), |_, _| {}, cx);
        }, window, cx);
    }

    // `user_id` is "@me" for our own nickname, as Discord's API spells it.
    fn edit_nickname(&mut self, guild_id: String, user_id: String, current: String, window: &mut Window, cx: &mut Context<Self>) {
        let title = if user_id == "@me" { "Change Nickname" } else { "Change Nickname" };
        self.prompt(title, "Everyone in this server sees this name.", &current, "Save", move |this, nick, _, cx| {
            let nick = (!nick.trim().is_empty()).then(|| nick.trim().to_owned());
            this.rest(Method::PATCH, format!("/guilds/{guild_id}/members/{user_id}"), Some(json!({ "nick": nick })), |_, _| {}, cx);
        }, window, cx);
    }

    fn set_ignored(&mut self, user_id: String, ignore: bool, cx: &mut Context<Self>) {
        let method = if ignore { Method::PUT } else { Method::DELETE };
        let id = user_id.clone();
        self.rest(method, format!("/users/@me/relationships/{user_id}/ignore"), None, move |this, _| {
            match ignore {
                true => this.ignored.insert(id),
                false => this.ignored.remove(&id),
            };
        }, cx);
    }

    // Opens (or creates) our DM with `user_id` and joins its call; it rings once connected.
    pub(crate) fn start_call(&mut self, user_id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        cx.spawn_in(window, async move |this, cx| {
            let Ok(dm) = client.open_dm(&user_id).await else { return };
            this.update_in(cx, |this, window, cx| {
                if !this.dms.iter().any(|d| d.id == dm.id) {
                    this.dms.insert(0, dm.clone());
                }
                this.guild_id = None;
                this.open_channel(dm.clone(), window, cx);
                this.ring_on_connect = Some(dm.id.clone());
                this.join_voice(dm, cx);
            })
            .ok();
        })
        .detach();
    }

    // Discord's "Invite to Server": a single-use invite to that server, sent to their DMs.
    fn invite_to_server(&mut self, guild_id: String, user_id: String, _: &mut Window, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        let Some(channel) = self.guilds.iter().find(|g| g.id == guild_id).and_then(|g| g.channels.iter().find(|c| c.kind == ChannelKind::Text).map(|c| c.id.clone())) else { return };
        cx.spawn(async move |this, cx| {
            let result = async {
                let invite = client.call(Method::POST, format!("/channels/{channel}/invites"), Some(json!({ "max_uses": 1, "unique": true }))).await?;
                let code = invite["code"].as_str().ok_or("Discord didn't return an invite")?.to_owned();
                let dm = client.open_dm(&user_id).await?;
                client.call(Method::POST, format!("/channels/{}/messages", dm.id), Some(json!({ "content": format!("https://discord.gg/{code}") }))).await?;
                Ok::<_, String>(())
            }
            .await;
            this.update(cx, |this, cx| match result {
                Ok(()) => this.show_toast("Invite sent", cx),
                Err(error) => this.show_toast(error, cx),
            })
            .ok();
        })
        .detach();
    }
}

fn toggle(list: &mut Vec<String>, id: &str) {
    match list.iter().position(|x| x == id) {
        Some(i) => {
            list.remove(i);
        }
        None => list.push(id.to_owned()),
    }
}
