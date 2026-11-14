// Discord sometimes answers a sign-in with "captcha-required" instead of a token. Wave has no web
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
<title>Wave sign-in</title>
<style>
  :root { color-scheme: light dark; --bg: #f2f3f5; --card: #fff; --text: #060607; --muted: #4e5058; --accent: #db2b65; --ok: #23a55a; --bad: #da373c; }
  @media (prefers-color-scheme: dark) { :root { --bg: #1e1f22; --card: #2b2d31; --text: #f2f3f5; --muted: #b5bac1; } }
  * { box-sizing: border-box; }
  body { margin: 0; min-height: 100vh; display: grid; place-items: center; background: var(--bg); color: var(--text); font: 15px/1.45 system-ui, -apple-system, "Segoe UI", sans-serif; padding: 16px; }
  main { background: var(--card); border-radius: 16px; padding: 32px 28px; max-width: 420px; width: 100%; text-align: center; box-shadow: 0 8px 24px rgb(0 0 0 / .12); }
  h1 { font-size: 20px; margin: 0 0 6px; }
  p { margin: 0 0 20px; color: var(--muted); }
  #box { display: flex; justify-content: center; min-height: 78px; }
  #status { margin: 16px 0 0; font-weight: 600; }
  #status.ok { color: var(--ok); } #status.bad { color: var(--bad); }
</style>
<script src="https://js.hcaptcha.com/1/api.js?onload=ready&render=explicit&recaptchacompat=off" async defer></script>
</head>
<body>
<main>
  <h1>One more step</h1>
  <p>Discord wants a quick check before signing Wave in.</p>
  <div id="box"></div>
  <div id="status"></div>
</main>
<script>
  const SITEKEY = {{SITEKEY}};
  const RQDATA = {{RQDATA}};
  const status = document.getElementById('status');
  function say(text, kind) { status.textContent = text; status.className = kind || ''; }
  async function solved(key) {
    say('Sending to Wave…');
    try {
      const res = await fetch(location.pathname + '/done', { method: 'POST', headers: { 'Content-Type': 'text/plain' }, body: key });
      if (!res.ok) throw new Error(res.status);
      say('Done. You can close this tab and go back to Wave.', 'ok');
    } catch (e) {
      say('Couldn’t reach Wave. Is it still open?', 'bad');
    }
  }
  function ready() {
    const id = hcaptcha.render('box', {
      sitekey: SITEKEY,
      theme: matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light',
      callback: solved,
      'expired-callback': () => say('The check expired. Solve it again.', 'bad'),
      'error-callback': (e) => say('Captcha error: ' + e, 'bad'),
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
