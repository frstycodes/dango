// Discord sometimes answers a sign-in with "captcha-required" instead of a token. Dango has no web
// view, so the challenge is shown in the user's own browser: a one-shot server on 127.0.0.1 serves a
// page with the hCaptcha widget, and the page posts the solved key back. The user solves it; nothing
// here tries to answer it.

use std::time::Duration;

use serde_json::Value;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const SOLVE_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// What Discord sent with its "captcha-required" response.
pub struct Challenge {
    pub sitekey: String,
    pub rqdata: Option<String>,
    pub rqtoken: Option<String>,
    pub session_id: Option<String>,
}

impl Challenge {
    pub fn from_response(body: &Value) -> Option<Self> {
        if body.get("captcha_key").is_none() || body["captcha_service"].as_str() != Some("hcaptcha") {
            return None;
        }
        let text = |key: &str| body[key].as_str().map(str::to_owned);
        Some(Self { sitekey: text("captcha_sitekey")?, rqdata: text("captcha_rqdata"), rqtoken: text("captcha_rqtoken"), session_id: text("captcha_session_id") })
    }

    /// Headers the retried request carries alongside the solved key.
    pub fn headers(&self, key: &str) -> Vec<(&'static str, String)> {
        let mut headers = vec![("X-Captcha-Key", key.to_owned())];
        if let Some(token) = &self.rqtoken {
            headers.push(("X-Captcha-Rqtoken", token.clone()));
        }
        if let Some(id) = &self.session_id {
            headers.push(("X-Captcha-Session-Id", id.clone()));
        }
        headers
    }
}

/// A started page: `url` is what to open in the browser, `solved` resolves with the hCaptcha key.
pub struct Pending {
    pub url: String,
    listener: TcpListener,
    secret: String,
    page: String,
}

pub async fn serve(challenge: &Challenge) -> Result<Pending, String> {
    let listener = TcpListener::bind("127.0.0.1:0").await.map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    // Unguessable path, so another local page can't post a key or read the challenge.
    let secret = format!("{:032x}", rand::random::<u128>());
    Ok(Pending { url: format!("http://127.0.0.1:{port}/{secret}"), listener, page: page(challenge), secret })
}

impl Pending {
    /// Serves the page until the browser posts the solved key, or gives up after five minutes.
    pub async fn solved(self) -> Result<String, String> {
        tokio::time::timeout(SOLVE_TIMEOUT, async {
            loop {
                let (stream, _) = self.listener.accept().await.map_err(|e| e.to_string())?;
                if let Some(key) = self.handle(stream).await {
                    return Ok(key);
                }
            }
        })
        .await
        .map_err(|_| "The captcha wasn't solved in time. Try again.".to_owned())?
    }

    async fn handle(&self, mut stream: TcpStream) -> Option<String> {
        let request = read_request(&mut stream).await?;
        let (head, body) = request.split_once("\r\n\r\n").unwrap_or((&request, ""));
        let line = head.lines().next().unwrap_or_default();
        let page_path = format!("/{}", self.secret);
        let done_path = format!("/{}/done", self.secret);
        let (status, content_type, reply, key) = match line.split_whitespace().take(2).collect::<Vec<_>>()[..] {
            ["GET", path] if path == page_path => ("200 OK", "text/html; charset=utf-8", self.page.clone(), None),
            ["POST", path] if path == done_path && !body.trim().is_empty() => ("200 OK", "text/plain", "ok".to_owned(), Some(body.trim().to_owned())),
            _ => ("404 Not Found", "text/plain", "not found".to_owned(), None),
        };
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{reply}",
            reply.len()
        );
        let _ = stream.write_all(response.as_bytes()).await;
        let _ = stream.shutdown().await;
        key
    }
}

// Reads one small HTTP request: the head, then as much body as Content-Length says (capped).
async fn read_request(stream: &mut TcpStream) -> Option<String> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = tokio::time::timeout(Duration::from_secs(10), stream.read(&mut chunk)).await.ok()?.ok()?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.len() > 64 * 1024 {
            return None;
        }
        let text = String::from_utf8_lossy(&buf);
        if let Some((head, body)) = text.split_once("\r\n\r\n") {
            let length = head
                .lines()
                .find_map(|l| l.split_once(':').filter(|(k, _)| k.eq_ignore_ascii_case("content-length")).map(|(_, v)| v.trim().parse::<usize>().unwrap_or(0)))
                .unwrap_or(0);
            if body.len() >= length {
                break;
            }
        }
    }
    String::from_utf8(buf).ok()
}

// Values come from Discord's response; JSON-encode them for the script and keep "</" out of it.
fn js(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".into()).replace("</", "<\\/")
}

fn page(challenge: &Challenge) -> String {
    let rqdata = challenge.rqdata.as_deref().map(js).unwrap_or_else(|| "null".into());
    PAGE.replace("{{SITEKEY}}", &js(&challenge.sitekey)).replace("{{RQDATA}}", &rqdata)
}

const PAGE: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Dango sign-in</title>
<style>
  /* Dango's look, from design/auth.html (.au-bpage, .au-ccard): the page on the sidebar tone, the card
     raised, Graphite neutrals following the system theme. Values mirror src/palette.rs. */
  :root { color-scheme: light dark; --page: #eceef0; --card: #fdfdfe; --line: rgb(43 46 50 / .10); --line-strong: rgb(43 46 50 / .18); --strong: #14161a; --muted: #55585e; --faint: #5d6166; --logo: #e25b26; --ok: #00723b; --bad: #b0182f; }
  @media (prefers-color-scheme: dark) { :root { --page: #111213; --card: #202224; --line: rgb(255 255 255 / .08); --line-strong: rgb(255 255 255 / .14); --strong: #f3f5f8; --muted: #a0a5ac; --faint: #8b8f97; --ok: #59d38c; --bad: #ff8068; } }
  * { box-sizing: border-box; corner-shape: superellipse(1.4); }
  .mascot i, .bars i { corner-shape: round; }
  body { margin: 0; min-height: 100vh; display: grid; place-items: center; padding: 32px 16px; background: var(--page); color: var(--muted); font: 400 14px/1.45 Gabarito, system-ui, -apple-system, "Segoe UI", sans-serif; -webkit-font-smoothing: antialiased; }
  main { width: 100%; max-width: 400px; display: grid; justify-items: center; gap: 12px; padding: 32px 24px; border-radius: 28px; background: var(--card); box-shadow: 0 0 0 1px var(--line); text-align: center; }
  h1 { margin: 0; color: var(--strong); font-size: 20px; font-weight: 700; line-height: 1.2; }
  p { margin: 0; text-wrap: pretty; }
  .mascot { display: flex; align-items: center; gap: 2px; height: 26px; }
  .mascot i { display: block; width: 3.2px; border-radius: 99px; background: var(--logo); animation: ripple 2.2s ease-in-out infinite; }
  .mascot i:nth-child(1) { height: 9.6px; } .mascot i:nth-child(2) { height: 18px; animation-delay: -.32s; } .mascot i:nth-child(3) { height: 25.6px; animation-delay: -.64s; } .mascot i:nth-child(4) { height: 15.6px; animation-delay: -.96s; } .mascot i:nth-child(5) { height: 8.8px; animation-delay: -1.28s; }
  .done .mascot i { animation: speak .9s ease-in-out infinite; }
  @keyframes ripple { 50% { transform: scaleY(.42); } }
  @keyframes speak { 50% { transform: scaleY(.35); } }
  /* The challenge takes its 304×78 slot from the start, so nothing jumps when it loads. */
  #box { width: 100%; max-width: 304px; min-height: 78px; display: flex; justify-content: center; }
  .slot { width: 100%; height: 78px; display: flex; align-items: center; gap: 12px; padding: 0 16px; border-radius: 11px; border: 1.5px dashed var(--line-strong); text-align: left; }
  .slot svg { flex: none; color: var(--faint); }
  .done #box { display: none; }
  #status { display: flex; align-items: center; justify-content: center; gap: 6px; min-height: 22px; font-weight: 600; }
  #status.ok { color: var(--ok); } #status.bad { color: var(--bad); }
  .bars { display: inline-flex; align-items: center; gap: 2px; height: 12px; }
  .bars i { width: 3px; height: 100%; border-radius: 9px; background: var(--logo); animation: speak .9s ease-in-out infinite; }
  .bars i:nth-child(1) { height: 55%; animation-delay: -.3s; } .bars i:nth-child(2) { animation-delay: -.6s; } .bars i:nth-child(3) { height: 70%; animation-delay: -.1s; }
  @media (prefers-reduced-motion: reduce) { .mascot i, .bars i { animation: none; } }
</style>
<script src="https://js.hcaptcha.com/1/api.js?onload=ready&render=explicit&recaptchacompat=off" async defer></script>
</head>
<body>
<main id="card">
  <div class="mascot" aria-hidden="true"><i></i><i></i><i></i><i></i><i></i></div>
  <h1 id="title">One more step</h1>
  <p id="why">Discord wants a quick check before signing Dango in.</p>
  <div id="box"><div class="slot"><svg width="22" height="22" viewBox="0 0 256 256" fill="currentColor" aria-hidden="true"><path d="M208,40H48A16,16,0,0,0,32,56v56c0,52.72,25.52,84.67,46.93,102.19,23.06,18.86,46,25.26,47,25.53a8,8,0,0,0,4.2,0c1-.27,23.91-6.67,47-25.53C198.48,196.67,224,164.72,224,112V56A16,16,0,0,0,208,40Zm0,72c0,37.07-13.66,67.16-40.6,89.42A129.3,129.3,0,0,1,128,223.62a128.25,128.25,0,0,1-38.92-21.81C61.82,179.51,48,149.3,48,112l0-56,160,0ZM82.34,141.66a8,8,0,0,1,11.32-11.32L112,148.69l50.34-50.35a8,8,0,0,1,11.32,11.32l-56,56a8,8,0,0,1-11.32,0Z"/></svg><span>The check loads here</span></div></div>
  <div id="status" role="status"></div>
</main>
<script>
  const SITEKEY = {{SITEKEY}};
  const RQDATA = {{RQDATA}};
  const status = document.getElementById('status');
  const BARS = '<span class="bars" aria-hidden="true"><i></i><i></i><i></i></span>';
  function say(text, kind, busy) { status.innerHTML = busy ? BARS : ''; status.append(text); status.className = kind || ''; }
  async function solved(key) {
    say('Sending to Dango…', '', true);
    try {
      const res = await fetch(location.pathname + '/done', { method: 'POST', headers: { 'Content-Type': 'text/plain' }, body: key });
      if (!res.ok) throw new Error(res.status);
      document.getElementById('card').classList.add('done');
      document.getElementById('title').textContent = 'You’re all set';
      document.getElementById('why').textContent = 'Dango is finishing your sign-in.';
      // GAP: the design's "Return to Dango" button needs a URL scheme the app doesn't register yet.
      say('Done. You can close this tab.', 'ok');
    } catch (e) {
      say('Couldn’t reach Dango. Make sure it’s still open, then solve the check again.', 'bad');
    }
  }
  function ready() {
    const box = document.getElementById('box');
    box.replaceChildren();
    const id = hcaptcha.render('box', {
      sitekey: SITEKEY,
      theme: matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light',
      callback: solved,
      'expired-callback': () => say('The check expired. Solve it again.', 'bad'),
      'error-callback': (e) => say('The check failed to load (' + e + '). Reload this page to try again.', 'bad'),
    });
    if (RQDATA) hcaptcha.setData(id, { rqdata: RQDATA });
  }
</script>
</body>
</html>
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_discord_challenge() {
        let body = json!({ "captcha_key": ["captcha-required"], "captcha_service": "hcaptcha", "captcha_sitekey": "k", "captcha_rqdata": "d", "captcha_rqtoken": "t", "captcha_session_id": "s" });
        let c = Challenge::from_response(&body).unwrap();
        assert_eq!(c.headers("key"), vec![("X-Captcha-Key", "key".into()), ("X-Captcha-Rqtoken", "t".into()), ("X-Captcha-Session-Id", "s".into())]);
        assert!(Challenge::from_response(&json!({ "message": "nope" })).is_none());
    }

    #[test]
    fn page_escapes_values() {
        let c = Challenge { sitekey: "a</script>".into(), rqdata: None, rqtoken: None, session_id: None };
        let html = page(&c);
        assert!(html.contains(r#"const SITEKEY = "a<\/script>";"#));
        assert!(html.contains("const RQDATA = null;"));
    }

    #[tokio::test]
    async fn serves_page_and_takes_key() {
        let c = Challenge { sitekey: "k".into(), rqdata: None, rqtoken: None, session_id: None };
        let pending = serve(&c).await.unwrap();
        let url = pending.url.clone();
        let solved = tokio::spawn(pending.solved());
        let client = reqwest::Client::new();
        assert!(client.get(format!("{url}x")).send().await.unwrap().status().is_client_error());
        assert!(client.get(&url).send().await.unwrap().text().await.unwrap().contains("hcaptcha"));
        client.post(format!("{url}/done")).body("P1_key").send().await.unwrap();
        assert_eq!(solved.await.unwrap().unwrap(), "P1_key");
    }
}
