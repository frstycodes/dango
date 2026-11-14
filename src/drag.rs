// Discord's drags in the channel list: people between voice channels (you, or anyone with Move
// Members), and channels to reorder them or move them between categories (Manage Channels).
// Both apply at once and roll back if Discord refuses.
use gpui_kit::{Context, FontWeight, IntoElement, Render, SharedString, Window, component::ActiveTheme, div, img, prelude::*, px};
use reqwest::Method;
use serde_json::json;

use crate::{
    Rustcord,
    discord::{Channel, ChannelKind, perm},
};

// Someone being dragged out of a voice channel.
#[derive(Clone)]
pub struct VoiceDrag {
    pub user_id: String,
    pub guild_id: String,
    pub name: SharedString,
    pub avatar: String,
}

impl Render for VoiceDrag {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        drag_pill(Some(self.avatar.clone()), self.name.clone(), cx)
    }
}

// A channel being dragged to a new place.
#[derive(Clone)]
pub struct ChannelDrag {
    pub channel_id: String,
    pub guild_id: String,
    pub name: SharedString,
}

impl Render for ChannelDrag {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        drag_pill(None, self.name.clone(), cx)
    }
}

fn drag_pill(avatar: Option<String>, name: SharedString, cx: &mut Context<impl Render>) -> impl IntoElement {
    let theme = cx.theme();
    div()
        .h(px(32.))
        .px_2()
        .rounded(px(8.))
        .bg(theme.popover)
        .border_1()
        .border_color(theme.input)
        .shadow_lg()
        .opacity(0.9)
        .flex()
        .items_center()
        .gap_2()
        .font_weight(FontWeight::MEDIUM)
        .children(avatar.map(|a| img(a).size(px(24.)).rounded_full()))
        .child(name)
}

// Where a dragged channel lands relative to the row it's dropped on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Place {
    Before,
    After,
    // Dropped on a category header: first in that category.
    Into,
}

impl Rustcord {
    pub(crate) fn can_move_people(&self, guild_id: &str) -> bool {
        let (Some(me), Some(guild)) = (self.me.as_ref(), self.guilds.iter().find(|g| g.id == guild_id)) else { return false };
        guild.can(&me.id, perm::MOVE_MEMBERS)
    }

    pub(crate) fn can_arrange_channels(&self, guild_id: &str) -> bool {
        let (Some(me), Some(guild)) = (self.me.as_ref(), self.guilds.iter().find(|g| g.id == guild_id)) else { return false };
        guild.can(&me.id, perm::MANAGE_CHANNELS)
    }

    // Drop someone on a voice channel: we join it ourselves, or move them there.
    pub(crate) fn drop_voice_user(&mut self, drag: &VoiceDrag, channel: Channel, cx: &mut Context<Self>) {
        let me = self.me.as_ref().is_some_and(|m| m.id == drag.user_id);
        if me {
            return self.join_voice(channel, cx);
        }
        let (guild_id, user_id, target) = (drag.guild_id.clone(), drag.user_id.clone(), channel.id.clone());
        let previous = self.voice_channel_of(&guild_id, &user_id);
        let (g1, u1, g2, u2) = (guild_id.clone(), user_id.clone(), guild_id.clone(), user_id.clone());
        self.rest_optimistic(
            Method::PATCH,
            format!("/guilds/{guild_id}/members/{user_id}"),
            Some(json!({ "channel_id": target })),
            move |this| this.set_voice_channel(&g1, &u1, Some(target)),
            move |this| this.set_voice_channel(&g2, &u2, previous),
            cx,
        );
    }

    fn voice_channel_of(&self, guild_id: &str, user_id: &str) -> Option<String> {
        self.guilds.iter().find(|g| g.id == guild_id)?.voice_states.iter().find(|v| v.user_id == user_id)?.channel_id.clone()
    }

    fn set_voice_channel(&mut self, guild_id: &str, user_id: &str, channel: Option<String>) {
        if let Some(state) = self.guilds.iter_mut().find(|g| g.id == guild_id).and_then(|g| g.voice_states.iter_mut().find(|v| v.user_id == user_id)) {
            state.channel_id = channel;
        }
    }

    // Drop a channel above or below another, or into a category: renumber that category's
    // channels and send Discord the new order in one request.
    pub(crate) fn drop_channel(&mut self, drag: &ChannelDrag, target: &Channel, place: Place, cx: &mut Context<Self>) {
        if drag.channel_id == target.id {
            return;
        }
        let Some(guild) = self.guilds.iter().find(|g| g.id == drag.guild_id) else { return };
        let Some(moved) = guild.channels.iter().find(|c| c.id == drag.channel_id).cloned() else { return };
        // Categories only move among categories; other channels land in the target's category.
        let parent = match (moved.kind, place) {
            (ChannelKind::Category, _) => None,
            (_, Place::Into) => Some(target.id.clone()),
            _ => target.parent_id.clone(),
        };
        let siblings: Vec<(String, i64)> = guild
            .channels
            .iter()
            .filter(|c| (c.kind == ChannelKind::Category) == (moved.kind == ChannelKind::Category) && (moved.kind == ChannelKind::Category || c.parent_id == parent))
            .map(|c| (c.id.clone(), c.position))
            .collect();
        let anchor = if place == Place::Into { None } else { Some((target.id.as_str(), place == Place::After)) };
        let order = reorder(siblings, &moved.id, anchor);
        let before: Vec<(String, i64, Option<String>)> = guild.channels.iter().map(|c| (c.id.clone(), c.position, c.parent_id.clone())).collect();
        let body: Vec<serde_json::Value> = order
            .iter()
            .map(|(id, position)| match id == &moved.id {
                true => json!({ "id": id, "position": position, "parent_id": parent, "lock_permissions": false }),
                false => json!({ "id": id, "position": position }),
            })
            .collect();
        let guild_id = drag.guild_id.clone();
        let (g1, g2, moved_id) = (guild_id.clone(), guild_id.clone(), moved.id.clone());
        self.rest_optimistic(
            Method::PATCH,
            format!("/guilds/{guild_id}/channels"),
            Some(json!(body)),
            move |this| {
                let Some(guild) = this.guilds.iter_mut().find(|g| g.id == g1) else { return };
                for channel in guild.channels.iter_mut() {
                    if let Some((_, position)) = order.iter().find(|(id, _)| *id == channel.id) {
                        channel.position = *position;
                    }
                    if channel.id == moved_id {
                        channel.parent_id = parent.clone();
                    }
                }
                guild.channels.sort_by_key(|c| c.position);
            },
            move |this| {
                let Some(guild) = this.guilds.iter_mut().find(|g| g.id == g2) else { return };
                for channel in guild.channels.iter_mut() {
                    if let Some((_, position, parent)) = before.iter().find(|(id, _, _)| *id == channel.id) {
                        channel.position = *position;
                        channel.parent_id = parent.clone();
                    }
                }
                guild.channels.sort_by_key(|c| c.position);
            },
            cx,
        );
    }
}

// The new order of a list of (id, position) after moving `moved` next to `anchor` (id, after?),
// or to the front when there's no anchor; positions are renumbered from 0.
pub fn reorder(mut siblings: Vec<(String, i64)>, moved: &str, anchor: Option<(&str, bool)>) -> Vec<(String, i64)> {
    siblings.sort_by_key(|(_, position)| *position);
    let mut ids: Vec<String> = siblings.into_iter().map(|(id, _)| id).filter(|id| id != moved).collect();
    let at = match anchor {
        Some((target, after)) => ids.iter().position(|id| id == target).map_or(ids.len(), |i| if after { i + 1 } else { i }),
        None => 0,
    };
    ids.insert(at.min(ids.len()), moved.to_owned());
    ids.into_iter().enumerate().map(|(i, id)| (id, i as i64)).collect()
}

// A drop's place from how far down the row the pointer is.
pub fn place_at(fraction: f32) -> Place {
    if fraction < 0.5 { Place::Before } else { Place::After }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(order: &[(String, i64)]) -> Vec<&str> {
        order.iter().map(|(id, _)| id.as_str()).collect()
    }

    #[test]
    fn reorders_and_renumbers() {
        let list = || vec![("a".to_owned(), 5), ("b".to_owned(), 1), ("c".to_owned(), 3)];
        assert_eq!(ids(&reorder(list(), "b", Some(("a", true)))), ["c", "a", "b"]);
        assert_eq!(ids(&reorder(list(), "a", Some(("b", false)))), ["a", "b", "c"]);
        assert_eq!(ids(&reorder(list(), "x", None)), ["x", "b", "c", "a"]);
        assert_eq!(reorder(list(), "c", Some(("b", false)))[0], ("c".to_owned(), 0));
        assert_eq!(place_at(0.2), Place::Before);
    }
}
