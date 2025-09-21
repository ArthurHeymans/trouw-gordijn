use std::{
    collections::VecDeque,
    net::{IpAddr, SocketAddr},
    process::Stdio,
    sync::{Arc, atomic::{AtomicU64, Ordering}},
    time::{Duration, Instant},
};

use axum::{
    extract::State,
    http::{header, StatusCode},
    response::{Html, IntoResponse},
    routing::{get, post},
    Form, Router,
};
use serde::Deserialize;
use tokio::{process::Command, sync::Mutex, time};
use tower_http::trace::TraceLayer;
use tracing::{error, info};

// Picture upload functionality removed

#[derive(Clone, Debug)]
struct AppConfig {
    bind_addr: SocketAddr,
    ssh_host: String,
    ssh_user: Option<String>,
    wled_host: String,
    wled_port: u16,
    local_tunnel_port: u16,
    text_param_key: Option<String>,
    text_preset_id: Option<i32>,
}

#[derive(Clone)]
struct AppState {
    cfg: AppConfig,
    client: reqwest::Client,
    // simple guard to avoid overlapping tunnel restarts
    tunnel_lock: Arc<Mutex<()>>,
    // message rotation
    queue: Arc<Mutex<VecDeque<QueuedMessage>>>,
    current: Arc<Mutex<Option<CurrentDisplay>>>,
    next_id: Arc<AtomicU64>,
}

#[derive(Clone, Debug)]
struct QueuedMessage {
    id: u64,
    text: String,
    color: Option<String>, // #rrggbb
    enqueued_at_ms: u128,
}

#[derive(Clone, Debug)]
struct CurrentDisplay {
    id: u64,
    text: String,
    color: Option<String>,
    started: Instant,
}

static INDEX_HTML_HEADER: &str = r##"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Trouw Gordijn</title>
  <style>
    :root { --gold:#d4af37; --rose:#f6d1c1; --ivory:#fffff0; --bg:#101014; --fg:#faf8f5; }
    html,body { margin:0; padding:0; background:var(--bg); color:var(--fg); font-family: system-ui, -apple-system, Segoe UI, Roboto, Ubuntu, Cantarell, Noto Sans, Helvetica, Arial, "Apple Color Emoji", "Segoe UI Emoji"; }
    .wrap { max-width: 860px; margin: 0 auto; padding: 24px; }
    .card { background: #161823; border:1px solid #2a2d3a; border-radius: 16px; padding: 20px; box-shadow: 0 10px 24px rgba(0,0,0,0.35); }
    h1 { font-weight: 700; letter-spacing: 0.5px; margin: 0 0 10px; }
    .accent { color: var(--gold); }
    .sub { color: #c9c6c2; margin-top:0; }
    .grid { display:grid; gap:18px; grid-template-columns: 1fr; }
    @media(min-width:900px){ .grid{ grid-template-columns: 1fr 1fr; } }
    label { display:block; font-weight:600; margin-bottom:8px; }
    input[type=text] { width:100%; padding:14px; border-radius:12px; border:1px solid #2a2d3a; background:#0e1017; color:var(--fg); font-size:16px; }
    input[type=color] { width: 56px; height: 40px; padding:0; border-radius:8px; border:1px solid #2a2d3a; background:#0e1017; }
    input[type=range] { width:100%; }
    button { background: linear-gradient(135deg, var(--gold), #ffde7a); color:#2d2200; font-weight:700; border: none; padding: 12px 18px; border-radius: 12px; cursor: pointer; box-shadow: 0 6px 18px rgba(212,175,55,0.35); }
    button:hover { filter: brightness(1.05); }
    .hero { display:flex; align-items:stretch; justify-content:stretch; background: radial-gradient(1200px 600px at 50% -10%, rgba(212,175,55,0.18), transparent); border-radius: 16px; overflow:hidden; padding: 0; }
    .hero img { width:100%; height:100%; object-fit:cover; display:block; }
    .brown { background: #4e342e; color: #fff3e0; border: 1px solid #6d4c41; }
    .queue-window { background:#4e342e; color:#fff3e0; padding:16px; width:100%; }
    .queue-title { font-weight:700; margin:0 0 10px; letter-spacing: .3px; }
    .queue-list { list-style:none; padding:0; margin:0; display:flex; flex-direction:column; gap:8px; }
    .queue-item { display:flex; align-items:center; gap:10px; padding:10px; border-radius:10px; background:#5d4037; border:1px solid rgba(0,0,0,0.15); }
    .queue-item.current { outline:2px solid #ffd180; background:#6d4c41; }
    .swatch { width:14px; height:14px; border-radius:3px; border:1px solid rgba(0,0,0,0.25); }
    .text { flex:1; white-space:nowrap; overflow:hidden; text-overflow:ellipsis; }
    .timer { font-variant-numeric: tabular-nums; opacity: .9; }
    .queue-empty { opacity:.8; font-style:italic; }
    .note { font-size: 12px; color: #a3a1a0; }
    .footer { text-align:center; color:#a3a1a0; margin-top:14px; font-size: 12px; }
    .row { display:flex; gap:12px; align-items:center; }
  </style>
  <script src="/assets/app.js" defer></script>
</head>
<body>
  <div class="wrap">
    <div class="card" style="margin-bottom:18px">
      <h1><span class="accent">Trouw</span> Gordijn <span class="note" style="margin-left:8px">UI v5</span></h1>
      <p class="sub">Laat je felicitatie schitteren op het LED gordijn ✨</p>
      <div class="grid">
        <div>
          <form onsubmit="submitMessage(event)">
            <label for="text">Jouw bericht</label>
            <input id="text" maxlength="64" required name="text" type="text" placeholder="Liefde, geluk en een lang leven samen!">
            <div style="height:12px"></div>
            <div>
              <label for="color">Kleur</label>
              <input id="color" type="hidden" value="#ffd700" name="color">
              <div class="row" style="align-items:flex-start">
                <div id="colorwheel" style="width:200px; height:200px; position:relative;">
                  <canvas id="wheelCanvas" width="200" height="200" style="border-radius:50%; cursor:crosshair; display:block;"></canvas>
                  <div id="pickerDot" style="position:absolute; width:12px; height:12px; border:2px solid #fff; border-radius:50%; left:94px; top:94px; pointer-events:none; box-shadow:0 0 2px rgba(0,0,0,.6);"></div>
                </div>
                <div id="colorPreview" title="Gekozen kleur" style="width:40px; height:40px; border-radius:8px; border:1px solid #2a2d3a; margin-left:12px; background:#ffd700"></div>
              </div>
            </div>
            <div style="height:16px"></div>
            <button type="submit">Stuur naar gordijn</button>
            <div class="note" style="margin-top:8px">Max 64 tekens. Houd het lief en feestelijk 💛</div>
          </form>
        </div>
        <div class="hero brown">
          <div class="queue-window">
            <h3 class="queue-title">Berichten wachtrij</h3>
            <ul id="queueList" class="queue-list">
              <li class="queue-empty">Geen berichten in de wachtrij…</li>
            </ul>
          </div>
        "##;

static INDEX_HTML_FOOTER: &str = r##"        </div>
      </div>
    </div>
    <div class=\"footer\">Met liefde gemaakt • Wens fijn en respectvol 💐</div>
  </div>
</body>
</html>"##;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cfg = load_config()?;
    // No uploads directory needed anymore

    let state = AppState {
        cfg: cfg.clone(),
        client: reqwest::Client::new(),
        tunnel_lock: Arc::new(Mutex::new(())),
        queue: Arc::new(Mutex::new(VecDeque::new())),
        current: Arc::new(Mutex::new(None)),
        next_id: Arc::new(AtomicU64::new(1)),
    };

    // Start tunnel supervision in background
    let tunnel_state = state.clone();
    tokio::spawn(async move { supervise_tunnel(tunnel_state).await });

    // Start message rotation worker
    let rot_state = state.clone();
    tokio::spawn(async move { rotation_worker(rot_state).await });

    let app = Router::new()
        .route("/", get(index))
        .route("/api/message", post(send_message))
        .route("/assets/app.js", get(app_js))
        .route("/api/queue", get(get_queue))
        .with_state(state)
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(cfg.bind_addr).await?;
    info!("listening on {}", cfg.bind_addr);
    axum::serve(listener, app).await?;
    Ok(())
}

fn load_config() -> anyhow::Result<AppConfig> {
    let bind_host = std::env::var("BIND_HOST").unwrap_or_else(|_| "0.0.0.0".into());
    let bind_port: u16 = std::env::var("BIND_PORT").ok().and_then(|s| s.parse().ok()).unwrap_or(8080);
    let ssh_host = std::env::var("SSH_HOST").unwrap_or_else(|_| "x220-nixos.tail19d694.ts.net".into());
    let ssh_user = std::env::var("SSH_USER").ok();
    let wled_host = std::env::var("WLED_HOST").unwrap_or_else(|_| "127.0.0.1".into()); // host as seen from SSH host
    let wled_port: u16 = std::env::var("WLED_PORT").ok().and_then(|s| s.parse().ok()).unwrap_or(80);
    let local_tunnel_port: u16 = std::env::var("LOCAL_TUNNEL_PORT").ok().and_then(|s| s.parse().ok()).unwrap_or(18080);
    let text_param_key = std::env::var("TEXT_PARAM_KEY").ok(); // e.g. "TT" if a Text usermod is installed
    let text_preset_id = std::env::var("TEXT_PRESET_ID").ok().and_then(|s| s.parse().ok());

    let ip: IpAddr = bind_host.parse().unwrap_or(IpAddr::from([0, 0, 0, 0]));
    Ok(AppConfig {
        bind_addr: SocketAddr::from((ip, bind_port)),
        ssh_host,
        ssh_user,
        wled_host,
        wled_port,
        local_tunnel_port,
        text_param_key,
        text_preset_id,
    })
}

async fn index(State(_state): State<AppState>) -> impl IntoResponse {
    let mut html = String::with_capacity(4096);
    html.push_str("<!-- UI_VERSION: wheel-v4 -->\n");
    html.push_str(INDEX_HTML_HEADER);
    html.push_str(INDEX_HTML_FOOTER);
    ([ (header::CACHE_CONTROL, "no-store, max-age=0"), (header::PRAGMA, "no-cache") ], Html(html))
}

static APP_JS: &str = r#"(function(){
  async function submitMessage(ev){
    ev.preventDefault();
    const fd = new FormData(ev.target);
    const res = await fetch('/api/message', { method: 'POST', body: new URLSearchParams(fd) });
    if(res.ok){ ev.target.reset(); try{ await refreshQueue(); }catch(e){} }
    else { const t = await res.text(); alert('Mislukt: '+t); }
  }
  window.submitMessage = submitMessage;

  function hsvToRgb(h, s, v){
    const c = v * s;
    const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
    const m = v - c;
    let r=0,g=0,b=0;
    if (0 <= h && h < 60) { r=c; g=x; b=0; }
    else if (60 <= h && h < 120) { r=x; g=c; b=0; }
    else if (120 <= h && h < 180) { r=0; g=c; b=x; }
    else if (180 <= h && h < 240) { r=0; g=x; b=c; }
    else if (240 <= h && h < 300) { r=x; g=0; b=c; }
    else { r=c; g=0; b=x; }
    return [Math.round((r+m)*255), Math.round((g+m)*255), Math.round((b+m)*255)];
  }
  function rgbToHsv(r, g, b){
    r /= 255; g /= 255; b /= 255;
    const max = Math.max(r,g,b), min = Math.min(r,g,b);
    const d = max - min;
    let h = 0;
    if (d !== 0){
      switch(max){
        case r: h = 60 * (((g-b)/d) % 6); break;
        case g: h = 60 * ((b-r)/d + 2); break;
        case b: h = 60 * ((r-g)/d + 4); break;
      }
    }
    if (h < 0) h += 360;
    const s = max === 0 ? 0 : d / max;
    const v = max;
    return { h, s, v };
  }
  function rgbToHex(r,g,b){ return '#' + [r,g,b].map(x => x.toString(16).padStart(2,'0')).join(''); }
  function hexToRgb(hex){ const m = /^#?([\da-f]{2})([\da-f]{2})([\da-f]{2})$/i.exec(hex); if(!m) return {r:255,g:215,b:0}; return { r: parseInt(m[1],16), g: parseInt(m[2],16), b: parseInt(m[3],16) }; }
  function drawColorWheel(canvas){
    const ctx = canvas.getContext('2d');
    const w = canvas.width, h = canvas.height;
    const R = Math.min(w,h)/2;
    const img = ctx.createImageData(w,h);
    const data = img.data;
    for(let y=0;y<h;y++){
      for(let x=0;x<w;x++){
        const dx = x - R;
        const dy = y - R;
        const d = Math.sqrt(dx*dx + dy*dy);
        const idx = (y*w + x)*4;
        if(d > R){ data[idx+3] = 0; continue; }
        const ang = Math.atan2(dy, dx);
        const hue = (ang * 180/Math.PI + 360) % 360;
        const sat = d / R;
        const [r,g,b] = hsvToRgb(hue, sat, 1);
        data[idx] = r; data[idx+1] = g; data[idx+2] = b; data[idx+3] = 255;
      }
    }
    ctx.putImageData(img, 0, 0);
  }
  function initColorWheel(){
    const canvas = document.getElementById('wheelCanvas');
    const hidden = document.getElementById('color');
    const dot = document.getElementById('pickerDot');
    const preview = document.getElementById('colorPreview');
    if(!canvas || !hidden || !dot) return;
    drawColorWheel(canvas);
    const R = canvas.width/2;
    function moveDotTo(hex){
      const {r,g,b} = hexToRgb(hex);
      const hsv = rgbToHsv(r,g,b);
      const rad = hsv.s * R;
      const x = R + rad * Math.cos(hsv.h * Math.PI/180);
      const y = R + rad * Math.sin(hsv.h * Math.PI/180);
      dot.style.left = (x - 6) + 'px';
      dot.style.top = (y - 6) + 'px';
      if(preview) preview.style.background = hex;
    }
    function pickAtPoint(x, y){
      const rect = canvas.getBoundingClientRect();
      const cx = x - rect.left; const cy = y - rect.top;
      const dx = cx - R; const dy = cy - R;
      const dist = Math.sqrt(dx*dx + dy*dy);
      if(dist > R) return;
      const d = canvas.getContext('2d').getImageData(Math.floor(cx), Math.floor(cy), 1, 1).data;
      if(d[3] === 0) return;
      const hex = rgbToHex(d[0], d[1], d[2]);
      hidden.value = hex;
      if(preview) preview.style.background = hex;
      dot.style.left = (cx - 6) + 'px';
      dot.style.top = (cy - 6) + 'px';
    }
    let dragging = false;
    canvas.addEventListener('mousedown', (e)=>{ dragging=true; pickAtPoint(e.clientX, e.clientY); });
    window.addEventListener('mousemove', (e)=>{ if(dragging) pickAtPoint(e.clientX, e.clientY); });
    window.addEventListener('mouseup', ()=> dragging=false);
    canvas.addEventListener('touchstart', (e)=>{ dragging=true; const t=e.touches[0]; pickAtPoint(t.clientX, t.clientY); e.preventDefault(); }, {passive:false});
    window.addEventListener('touchmove', (e)=>{ if(dragging){ const t=e.touches[0]; pickAtPoint(t.clientX, t.clientY);} }, {passive:false});
    window.addEventListener('touchend', ()=> dragging=false);
    moveDotTo(hidden.value || '#ffd700');
  }
  async function refreshQueue(){
    const ul = document.getElementById('queueList');
    if(!ul) return;
    const r = await fetch('/api/queue', { cache: 'no-store' });
    const data = await r.json();
    ul.innerHTML = '';
    if(!data.current && (!data.items || data.items.length===0)){
      const li = document.createElement('li'); li.className='queue-empty'; li.textContent='Geen berichten in de wachtrij…'; ul.appendChild(li); return;
    }
    const renderItem = (item, isCurrent, elapsed) => {
      const li = document.createElement('li'); li.className='queue-item'+(isCurrent?' current':'');
      const sw = document.createElement('span'); sw.className='swatch'; sw.style.background = item.color || '#ffd700'; li.appendChild(sw);
      const text = document.createElement('span'); text.className='text'; text.textContent=item.text; li.appendChild(text);
      if(isCurrent){ const t = document.createElement('span'); t.className='timer'; const s = Math.max(0, Math.min(60, Math.floor(elapsed||0))); t.textContent = '⏱ ' + String(s).padStart(2,'0')+'s'; t.setAttribute('data-elapsed', String(s)); li.appendChild(t); }
      return li;
    };
    if(data.current){ ul.appendChild(renderItem(data.current, true, data.elapsed_seconds)); }
    for(const it of (data.items||[])){ ul.appendChild(renderItem(it, false)); }
  }

  function tickTimer(){
    const t = document.querySelector('.queue-item.current .timer');
    if(!t) return; let s = parseInt(t.getAttribute('data-elapsed')||'0',10); s = Math.min(60, s+1); t.setAttribute('data-elapsed', String(s)); t.textContent='⏱ ' + String(s).padStart(2,'0')+'s';
  }

  function boot(){
    initColorWheel();
    refreshQueue();
    setInterval(()=>{ refreshQueue().catch(()=>{}); }, 5000);
    setInterval(tickTimer, 1000);
  }

  if (document.readyState !== 'loading') boot(); else window.addEventListener('DOMContentLoaded', boot);
})();"#;

async fn app_js() -> impl IntoResponse {
    ([
        (header::CONTENT_TYPE, "application/javascript; charset=utf-8"),
        (header::CACHE_CONTROL, "no-store, max-age=0"),
        (header::PRAGMA, "no-cache"),
    ], APP_JS)
}

#[derive(Deserialize, Debug)]
struct MessageForm {
    text: String,
    color: Option<String>, // #rrggbb
}

async fn send_message(State(state): State<AppState>, Form(form): Form<MessageForm>) -> impl IntoResponse {
    let text = form.text.trim().to_string();
    if text.is_empty() || text.len() > 128 {
        return (StatusCode::BAD_REQUEST, "Invalid text").into_response();
    }

    let id = state.next_id.fetch_add(1, Ordering::Relaxed);

    // If only the last item is showing and it already ran 60s, jump to the new one immediately
    let mut switched_now = false;
    {
        let mut cur = state.current.lock().await;
        if let Some(ref display) = *cur {
            if display.started.elapsed() >= Duration::from_secs(60) {
                let new_disp = CurrentDisplay { id, text: text.clone(), color: form.color.clone(), started: Instant::now() };
                *cur = Some(new_disp.clone());
                switched_now = true;
                drop(cur);
                if let Err(e) = apply_display(&state, &new_disp.text, new_disp.color.as_deref()).await { error!(?e, "apply_display failed"); }
            }
        }
    }

    if !switched_now {
        // Enqueue for rotation
        let mut q = state.queue.lock().await;
        q.push_back(QueuedMessage { id, text, color: form.color, enqueued_at_ms: now_ms() });
    }
    (StatusCode::OK, if switched_now { "switched" } else { "queued" }).into_response()
}

// Upload endpoint removed

fn parse_hex_color(s: &str) -> Option<(u8, u8, u8)> {
    let s = s.strip_prefix('#').unwrap_or(s);
    if s.len() != 6 { return None; }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some((r,g,b))
}

async fn supervise_tunnel(state: AppState) {
    loop {
        if let Err(e) = ensure_tunnel(&state).await {
            error!(?e, "ssh tunnel error");
            time::sleep(Duration::from_secs(5)).await;
            continue;
        }
        // Keep a light heartbeat to WLED through the tunnel
        let base = format!("http://127.0.0.1:{}", state.cfg.local_tunnel_port);
        let res = state.client.get(format!("{}/json", base)).send().await;
        match res {
            Ok(_) => time::sleep(Duration::from_secs(10)).await,
            Err(_) => time::sleep(Duration::from_secs(2)).await,
        }
    }
}

async fn ensure_tunnel(state: &AppState) -> anyhow::Result<()> {
    let _g = state.tunnel_lock.lock().await; // serialize restarts
    // quick probe if local port already responds
    let base = format!("http://127.0.0.1:{}", state.cfg.local_tunnel_port);
    if state.client.get(format!("{}/", base)).send().await.is_ok() {
        return Ok(());
    }

    // Start ssh -NT -L 127.0.0.1:<local>:<wled_host>:<wled_port> <ssh_target>
    let mut target = String::new();
    if let Some(user) = &state.cfg.ssh_user { target.push_str(user); target.push('@'); }
    target.push_str(&state.cfg.ssh_host);

    let forward = format!(
        "127.0.0.1:{}:{}:{}",
        state.cfg.local_tunnel_port, state.cfg.wled_host, state.cfg.wled_port
    );

    info!("starting ssh tunnel to {} forwarding {}", target, forward);
    let mut cmd = Command::new("ssh");
    cmd.arg("-NT")
        .arg("-o").arg("ExitOnForwardFailure=yes")
        .arg("-o").arg("ServerAliveInterval=10")
        .arg("-o").arg("ServerAliveCountMax=3")
        .arg("-L").arg(forward)
        .arg(target)
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    // launch in background and give it a moment
    let _child = cmd.spawn()?; // intentionally not awaited; relies on autossh-like keepalive via supervise
    time::sleep(Duration::from_millis(400)).await;
    Ok(())
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

async fn rotation_worker(state: AppState) {
    loop {
        // If no current and something is queued, start it
        {
            let mut cur = state.current.lock().await;
            if cur.is_none() {
                let mut q = state.queue.lock().await;
                if let Some(next) = q.pop_front() {
                    let display = CurrentDisplay {
                        id: next.id,
                        text: next.text.clone(),
                        color: next.color.clone(),
                        started: Instant::now(),
                    };
                    *cur = Some(display.clone());
                    drop(q);
                    drop(cur);
                    if let Err(e) = apply_display(&state, &display.text, display.color.as_deref()).await {
                        error!(?e, "apply_display failed");
                    }
                }
            }
        }

        // If current elapsed >= 60s, advance to next if present; otherwise keep displaying last
        {
            let mut maybe_new_display: Option<CurrentDisplay> = None;
            {
                let mut cur = state.current.lock().await;
                if let Some(ref display) = *cur {
                    if display.started.elapsed() >= Duration::from_secs(60) {
                        let mut q = state.queue.lock().await;
                        if let Some(next) = q.pop_front() {
                            let new_disp = CurrentDisplay {
                                id: next.id,
                                text: next.text.clone(),
                                color: next.color.clone(),
                                started: Instant::now(),
                            };
                            *cur = Some(new_disp.clone());
                            maybe_new_display = Some(new_disp);
                        } else {
                            // No next item; keep showing current as-is (the last item stays)
                        }
                    }
                }
            }
            if let Some(d) = maybe_new_display {
                if let Err(e) = apply_display(&state, &d.text, d.color.as_deref()).await { error!(?e, "apply_display failed"); }
            }
        }
        time::sleep(Duration::from_millis(900)).await;
    }
}

async fn apply_display(state: &AppState, text: &str, color: Option<&str>) -> anyhow::Result<()> {
    if let Err(e) = ensure_tunnel(state).await { error!(?e, "tunnel ensure failed"); }
    let base = format!("http://127.0.0.1:{}", state.cfg.local_tunnel_port);

    let (r, g, b) = color.and_then(parse_hex_color).unwrap_or((255, 215, 0));
    let bri: u8 = 128;
    let json_body = serde_json::json!({
        "on": true,
        "bri": bri,
        "seg": [{ "id": 0, "n": text, "col": [[r, g, b]] }]
    });
    let _ = state.client.post(format!("{}/json/state", base)).json(&json_body).send().await;

    if let Some(ps) = state.cfg.text_preset_id {
        let _ = state.client.post(format!("{}/json/state", base)).json(&serde_json::json!({"ps": ps})).send().await;
    }

    if let Some(key) = &state.cfg.text_param_key {
        let url = format!("{}/win?{}={}", base, key, urlencoding::encode(text));
        let _ = state.client.get(url).send().await;
    }
    Ok(())
}

async fn get_queue(State(state): State<AppState>) -> impl IntoResponse {
    let cur = state.current.lock().await;
    let (current, elapsed) = if let Some(ref c) = *cur {
        (Some(serde_json::json!({
            "id": c.id,
            "text": c.text,
            "color": c.color,
        })), c.started.elapsed().as_secs())
    } else { (None, 0) };
    drop(cur);
    let q = state.queue.lock().await;
    let items: Vec<_> = q.iter().map(|m| serde_json::json!({
        "id": m.id,
        "text": m.text,
        "color": m.color,
    })).collect();
    let body = serde_json::json!({
        "current": current,
        "elapsed_seconds": elapsed,
        "items": items,
    });
    ([ (header::CACHE_CONTROL, "no-store, max-age=0"), (header::PRAGMA, "no-cache"), (header::CONTENT_TYPE, "application/json") ], body.to_string())
}
