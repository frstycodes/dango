// Rich embeds: renders Discord's embed objects (the API's JSON, unchanged) the way Waffle draws them.
// Images are placeholders hued from their URL; this static site has no network.

const DIRECTORY = {
  users: { "454188684002983946": "frsty", "80351110224678912": "Mira", "155149108183695360": "Dyno" },
  roles: { "9001": ["Moderator", "#3ba55d"], "9002": ["Raid lead", "#e67e22"] },
  channels: { "1200000000000000001": ["g", "voice"], "1200000000000000002": ["general", "text"], "1200000000000000003": ["mod-log", "text"] },
};

const LIMITS = { title: 256, description: 4096, fields: 25, fieldName: 256, fieldValue: 1024, footer: 2048, author: 256, total: 6000, embeds: 10 };

const esc = s => String(s).replace(/[&<>"]/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
const hue = s => [...String(s)].reduce((h, c) => (h * 31 + c.charCodeAt(0)) % 360, 7);
const hex = n => "#" + n.toString(16).padStart(6, "0");

// Discord markdown, the subset embeds accept: headers, subtext, quotes, lists, code, masked links, mentions, timestamps.
function md(src, { block = true } = {}) {
  const kept = [];
  const keep = html => `\u0000${kept.push(html) - 1}\u0000`;
  const keepBlock = html => `\u0001${kept.push(html) - 1}\u0001`;
  let s = esc(src);
  s = s.replace(/```(?:[\w+-]*\n)?([\s\S]*?)```/g, (_, code) => keepBlock(`<pre class="rcode">${code.replace(/^\n|\n$/g, "")}</pre>`));
  s = s.replace(/`([^`\n]+)`/g, (_, code) => keep(`<code>${code}</code>`));
  s = s.replace(/\[([^\]\n]+)\]\((https?:\/\/[^)\s]+)\)/g, (_, text, url) => keep(`<a href="${url}" title="${url}">${inline(text)}</a>`));
  s = s.replace(/https?:\/\/[^\s<]+[^\s<.,:;)"'\]]/g, url => keep(`<a href="${url}">${url}</a>`));
  s = s.replace(/&lt;@!?(\d+)&gt;/g, (_, id) => keep(`<span class="at">@${esc(DIRECTORY.users[id] ?? "unknown-user")}</span>`));
  s = s.replace(/&lt;@&amp;(\d+)&gt;/g, (_, id) => { const [name, color] = DIRECTORY.roles[id] ?? ["unknown-role", "#99aab5"]; return keep(`<span class="at at--role" style="--role: ${color}">@${esc(name)}</span>`); });
  s = s.replace(/&lt;#(\d+)&gt;/g, (_, id) => { const [name, kind] = DIRECTORY.channels[id] ?? ["unknown", "text"]; return keep(`<span class="at"><i class="ph-fill ${kind === "voice" ? "ph-speaker-high" : "ph-hash"}" aria-hidden="true"></i>${esc(name)}</span>`); });
  s = s.replace(/&lt;t:(-?\d+)(?::([tTdDfFR]))?&gt;/g, (_, unix, style) => keep(stamp(+unix * 1000, style ?? "f")));
  s = inline(s);
  if (block) s = blocks(s);
  return s.replace(/[\u0000\u0001](\d+)[\u0000\u0001]/g, (_, i) => kept[i]);
}

function inline(s) {
  return s
    .replace(/\*\*\*(.+?)\*\*\*/g, "<b><i>$1</i></b>")
    .replace(/\*\*(.+?)\*\*/g, "<b>$1</b>")
    .replace(/__(.+?)__/g, "<u>$1</u>")
    .replace(/(^|[^\w*])\*(?!\s)(.+?)\*(?!\*)/g, "$1<i>$2</i>")
    .replace(/(^|\W)_(?!\s)(.+?)_(?=\W|$)/g, "$1<i>$2</i>")
    .replace(/~~(.+?)~~/g, "<s>$1</s>")
    .replace(/\|\|(.+?)\|\|/g, '<span class="spoiler" role="button" tabindex="0" aria-pressed="false" aria-label="Spoiler, press to reveal"><span>$1</span></span>');
}

function blocks(s) {
  const out = [];
  const lines = s.split("\n");
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    if (line.startsWith("&gt;&gt;&gt; ")) { out.push(`<blockquote>${blocks([line.slice(13), ...lines.slice(i + 1)].join("\n"))}</blockquote>`); break; }
    if (/^&gt; ?/.test(line)) {
      const quoted = [];
      while (i < lines.length && /^&gt; ?/.test(lines[i])) quoted.push(lines[i++].replace(/^&gt; ?/, ""));
      i--; out.push(`<blockquote>${quoted.join("<br>")}</blockquote>`); continue;
    }
    const list = line.match(/^(\s*)([-*]|\d+\.) (.*)/);
    if (list) {
      const ordered = /\d/.test(list[2]), items = [];
      while (i < lines.length && /^\s*([-*]|\d+\.) /.test(lines[i])) items.push(lines[i++].replace(/^\s*([-*]|\d+\.) /, ""));
      i--; const tag = ordered ? "ol" : "ul";
      out.push(`<${tag}>${items.map(x => `<li>${x}</li>`).join("")}</${tag}>`); continue;
    }
    const head = line.match(/^(#{1,3}) (.*)/);
    if (head) { out.push(`<div class="md-h${head[1].length}">${head[2]}</div>`); continue; }
    if (line.startsWith("-# ")) { out.push(`<div class="md-sub">${line.slice(3)}</div>`); continue; }
    // A blank line right after a block (heading, list, quote, code) would double its own spacing.
    if (!line && /^<(div|ul|ol|blockquote)|\u0001$/.test(out.at(-1) ?? "")) continue;
    out.push(line + (i < lines.length - 1 && !/^\u0001\d+\u0001$/.test(line) ? "<br>" : ""));
  }
  return out.join("");
}

// <t:…> styles and the footer timestamp, in the reader's locale.
function stamp(ms, style) {
  const d = new Date(ms);
  const full = d.toLocaleString(undefined, { dateStyle: "full", timeStyle: "short" });
  const fmt = {
    t: { timeStyle: "short" }, T: { timeStyle: "medium" }, d: { dateStyle: "short" }, D: { dateStyle: "long" },
    f: { dateStyle: "long", timeStyle: "short" }, F: { dateStyle: "full", timeStyle: "short" },
  }[style];
  const text = style === "R" ? relative(ms) : d.toLocaleString(undefined, fmt);
  return `<time class="rts" datetime="${d.toISOString()}" title="${full}">${text}</time>`;
}

function relative(ms) {
  const rtf = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });
  const sec = (ms - Date.now()) / 1000;
  for (const [unit, size] of [["year", 31536000], ["month", 2592000], ["day", 86400], ["hour", 3600], ["minute", 60]]) if (Math.abs(sec) >= size) return rtf.format(Math.round(sec / size), unit);
  return rtf.format(Math.round(sec), "second");
}

function footerTime(iso) {
  const d = new Date(iso), now = new Date();
  const time = d.toLocaleTimeString(undefined, { timeStyle: "short" });
  const days = Math.round((new Date(now.toDateString()) - new Date(d.toDateString())) / 86400000);
  const text = days === 0 ? `Today at ${time}` : days === 1 ? `Yesterday at ${time}` : `${d.toLocaleDateString(undefined, { dateStyle: "short" })} ${time}`;
  return `<time datetime="${d.toISOString()}" title="${d.toLocaleString(undefined, { dateStyle: "full", timeStyle: "short" })}">${text}</time>`;
}

const icon = (url, name) => url ? `<span class="av" style="--hue: ${hue(url)}" aria-hidden="true">${esc(name?.[0] ?? "")}</span>` : "";
const aspect = m => m?.width && m?.height ? `${m.width} / ${m.height}` : null;
const pic = (m, extra = "") => `<div class="pic" role="img" aria-label="${esc(m.description ?? "Image")}" style="--h: ${hue(m.url)};${aspect(m) ? ` --ar: ${aspect(m)};` : ""}${extra}"></div>`;
const isSensitive = e => (e.flags ?? 0) & ((1 << 4) | (1 << 6) | (1 << 7));

// Consecutive inline fields share a row: three across, two when a thumbnail takes the right edge.
function fieldRows(fields, hasThumb) {
  const per = hasThumb ? 2 : 3, rows = [];
  let run = [];
  const flush = () => { if (run.length) rows.push(run); run = []; };
  for (const f of fields) {
    if (!f.inline) { flush(); rows.push([f]); continue; }
    run.push(f);
    if (run.length === per) flush();
  }
  flush();
  return rows;
}

function media(e, gallery) {
  const video = e.type === "video" || e.video;
  if (gallery.length > 1) {
    const pics = gallery.slice(0, 4);
    return `<div class="gallery g${pics.length}">${pics.map(m => pic(m)).join("")}</div>`;
  }
  const m = e.image ?? (video ? e.video ?? e.thumbnail : null);
  if (!m) return "";
  const play = video ? `<button class="play" aria-label="Play ${esc(e.title ?? "video")}"><i class="ph-fill ph-play"></i></button>` : "";
  return video ? pic(m).replace("></div>", `>${play}</div>`) : pic(m);
}

function sensitiveWrap(html, e) {
  if (!isSensitive(e)) return html;
  return `<div class="rsens">${html}<button type="button" data-reveal><i class="ph-fill ph-eye-slash" aria-hidden="true"></i>Sensitive content · Show</button></div>`;
}

function renderEmbed(e, gallery = []) {
  if (e.type === "image" || e.type === "gifv") {
    const m = e.type === "gifv" ? e.video ?? e.thumbnail : e.thumbnail ?? e.image;
    const tag = e.type === "gifv" ? `<span class="media-tag tl">GIF</span>` : "";
    return sensitiveWrap(`<div class="pic rbare" role="img" aria-label="${esc(m?.description ?? "Image")}" style="--h: ${hue(m?.url ?? e.url)};${aspect(m) ? ` --ar: ${aspect(m)}; --w: ${Math.min(m.width, 400)}px;` : ""}">${tag}</div>`, e);
  }
  if (e.type === "poll_result") return renderPollResult(e);

  const video = e.type === "video" || e.video;
  const thumb = !video && e.thumbnail ? e.thumbnail : null;
  const color = Number.isInteger(e.color) ? hex(e.color) : null;
  const text = [];
  if (e.provider?.name) text.push(`<div class="rprov">${e.provider.url ? `<a href="${esc(e.provider.url)}" style="color:inherit;text-decoration:none">${esc(e.provider.name)}</a>` : esc(e.provider.name)}</div>`);
  if (e.author?.name) {
    const name = e.author.url ? `<a class="rauth-name" href="${esc(e.author.url)}">${esc(e.author.name)}</a>` : `<span>${esc(e.author.name)}</span>`;
    text.push(`<div class="rauth">${icon(e.author.icon_url, e.author.name)}${name}</div>`);
  }
  if (e.title) text.push(e.url ? `<a class="rtitle" href="${esc(e.url)}">${md(e.title, { block: false })}</a>` : `<div class="rtitle">${md(e.title, { block: false })}</div>`);
  if (e.description) text.push(`<div class="body rdesc">${md(e.description)}</div>`);
  if (e.fields?.length) {
    const rows = fieldRows(e.fields, !!thumb);
    text.push(`<div class="rfields">${rows.map(row => `<div class="rrow" style="--n: ${row.length}">${row.map(f => `<div class="rf"><div class="rf-name">${md(f.name, { block: false })}</div><div class="body">${md(f.value)}</div></div>`).join("")}</div>`).join("")}</div>`);
  }
  const parts = [];
  if (text.length) parts.push(`<div class="rtext">${text.join("")}</div>`);
  if (thumb) parts.push(`<div class="pic rthumb" role="img" aria-label="${esc(thumb.description ?? "Thumbnail")}" style="--h: ${hue(thumb.url)}"></div>`);
  const m = media(e, gallery);
  if (m) parts.push(`<div class="rmedia">${sensitiveWrap(m, e)}</div>`);
  if (e.footer?.text || e.timestamp) {
    const bits = [e.footer?.text && esc(e.footer.text), e.timestamp && footerTime(e.timestamp)].filter(Boolean);
    parts.push(`<footer class="rfoot">${icon(e.footer?.icon_url, e.footer?.text)}<span>${bits.join(" · ")}</span></footer>`);
  }
  return `<article class="remb"${color ? ` style="--c: ${color}"` : ""}>${parts.join("")}</article>`;
}

function renderPollResult(e) {
  const f = Object.fromEntries((e.fields ?? []).map(x => [x.name, x.value]));
  const votes = +f.victor_answer_votes || 0, total = +f.total_votes || 0;
  const pct = total ? Math.round((votes / total) * 100) : 0;
  const won = f.victor_answer_text
    ? `<b>${esc(f.victor_answer_text)}</b><small><i class="ph-fill ph-check-circle" aria-hidden="true"></i>Won with ${votes.toLocaleString()} ${votes === 1 ? "vote" : "votes"} · ${pct}%</small>`
    : `<b>No winner</b><small>${total ? "Tied at the top" : "Nobody voted"}</small>`;
  return `<div class="rpoll"><span class="e" aria-hidden="true">${esc(f.victor_answer_emoji_name ?? "📊")}</span><div style="min-width:0">${won}</div><button class="btn btn--sm">View poll</button></div>`;
}

// Embeds sharing a URL fold into the first one's media as a gallery, up to four pictures.
function renderEmbeds(embeds) {
  const out = [], byUrl = new Map();
  for (const e of embeds.slice(0, LIMITS.embeds)) {
    const first = e.url && byUrl.get(e.url);
    if (first) { if (e.image) first.gallery.push(e.image); continue; }
    const entry = { e, gallery: e.image ? [e.image] : [] };
    if (e.url) byUrl.set(e.url, entry);
    out.push(entry);
  }
  return out.map(({ e, gallery }) => renderEmbed(e, gallery)).join("");
}

// Limits, measured the way the API counts them (trimmed, combined across the message's embeds).
function checkLimits(embeds) {
  const len = s => (s ?? "").trim().length;
  const issues = [];
  let total = 0;
  if (embeds.length > LIMITS.embeds) issues.push(`${embeds.length} embeds, the most is ${LIMITS.embeds}`);
  embeds.forEach((e, i) => {
    const at = embeds.length > 1 ? `Embed ${i + 1}: ` : "";
    const over = (what, n, max) => n > max && issues.push(`${at}${what} is ${n.toLocaleString()} characters, the most is ${max.toLocaleString()}`);
    over("title", len(e.title), LIMITS.title);
    over("description", len(e.description), LIMITS.description);
    over("author name", len(e.author?.name), LIMITS.author);
    over("footer text", len(e.footer?.text), LIMITS.footer);
    if ((e.fields?.length ?? 0) > LIMITS.fields) issues.push(`${at}${e.fields.length} fields, the most is ${LIMITS.fields}`);
    for (const [j, f] of (e.fields ?? []).entries()) {
      over(`field ${j + 1} name`, len(f.name), LIMITS.fieldName);
      over(`field ${j + 1} value`, len(f.value), LIMITS.fieldValue);
      if (!len(f.name) || !len(f.value)) issues.push(`${at}field ${j + 1} needs both a name and a value`);
    }
    total += len(e.title) + len(e.description) + len(e.author?.name) + len(e.footer?.text) + (e.fields ?? []).reduce((n, f) => n + len(f.name) + len(f.value), 0);
  });
  if (total > LIMITS.total) issues.push(`${total.toLocaleString()} characters across the message's embeds, the most is ${LIMITS.total.toLocaleString()}`);
  return { total, issues };
}

// Long descriptions clip once rendered, so the measurement is the real one.
function clipLong(scope) {
  for (const d of scope.querySelectorAll(".rdesc:not([data-clipped])")) {
    d.dataset.clipped = "";
    if (d.scrollHeight <= 300) continue;
    d.classList.add("rclip");
    d.insertAdjacentHTML("afterend", `<button type="button" class="rmore" aria-expanded="false">Show more</button>`);
  }
}

document.addEventListener("click", e => {
  const more = e.target.closest(".rmore");
  if (more) {
    const open = more.getAttribute("aria-expanded") !== "true";
    more.previousElementSibling.classList.toggle("rclip", !open);
    more.setAttribute("aria-expanded", open);
    more.textContent = open ? "Show less" : "Show more";
  }
  const reveal = e.target.closest("[data-reveal]");
  if (reveal) reveal.parentElement.classList.add("shown");
});

function mount(el, embeds) {
  el.innerHTML = renderEmbeds(embeds);
  clipLong(el);
}

window.Embeds = { mount, checkLimits, LIMITS };

for (const el of document.querySelectorAll("[data-embeds]")) mount(el, EXAMPLES[el.dataset.embeds]);
