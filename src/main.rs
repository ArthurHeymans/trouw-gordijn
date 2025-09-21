use std::{
    collections::VecDeque,
    net::{IpAddr, SocketAddr},
    process::Stdio,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
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
    // ACME/HTTPS (only when feature enabled)
    #[cfg(feature = "acme")]
    acme_domain: Option<String>,
    #[cfg(feature = "acme")]
    acme_contact_email: Option<String>,
    #[cfg(feature = "acme")]
    acme_cache_dir: String,
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
    .lang { float:right; display:flex; gap:6px; }
    .lang button { background:#2a2d3a; color:#fff; border:none; padding:6px 8px; border-radius:8px; cursor:pointer; font-size:18px; line-height:1; }
    .lang button.active { outline:2px solid var(--gold); }
  </style>
  <script src="/assets/app.js" defer></script>
</head>
<body>
  <div class="wrap">
    <div class="card" style="margin-bottom:18px">
      <div class="lang" id="langSelector">
        <button type="button" data-lang="nl" aria-label="Nederlands">🇳🇱</button>
        <button type="button" data-lang="fr" aria-label="Français">🇫🇷</button>
        <button type="button" data-lang="de" aria-label="Deutsch">🇩🇪</button>
      </div>
      <h1><span class="accent">Trouw</span> Gordijn <span class="note" style="margin-left:8px">UI v5</span></h1>
      <p class="sub" data-i18n="subtitle">Laat je felicitatie schitteren op het LED gordijn ✨</p>
      <div class="grid">
        <div>
          <form onsubmit="submitMessage(event)">
            <label for="text" data-i18n="message_label">Jouw bericht</label>
            <input id="text" maxlength="64" required name="text" type="text" placeholder="Liefde, geluk en een lang leven samen!">
            <div style="height:12px"></div>
            <div>
              <label for="color" data-i18n="color_label">Kleur</label>
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
            <button type="submit" data-i18n="submit_btn">Stuur naar gordijn</button>
            <div class="note" style="margin-top:8px" data-i18n="note">Max 64 tekens. Houd het lief en feestelijk 💛</div>
          </form>
        </div>
        <div class="hero brown">
          <div class="queue-window">
            <h3 class="queue-title" data-i18n="queue_title">Berichten wachtrij</h3>
            <ul id="queueList" class="queue-list">
              <li class="queue-empty" data-i18n="queue_empty">Geen berichten in de wachtrij…</li>
            </ul>
          </div>
        "##;

static INDEX_HTML_FOOTER: &str = r##"        </div>
      </div>
    </div>
    <div class="footer" data-i18n="footer">Met liefde gemaakt • Wens fijn en respectvol 💐</div>
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
        .route("/admin", get(admin_page))
        .route("/api/message", post(send_message))
        .route("/assets/app.js", get(app_js))
        .route("/api/queue", get(get_queue))
        .route("/api/admin/remove", post(admin_remove))
        .with_state(state)
        .layer(TraceLayer::new_for_http());

    // If ACME is configured and the feature is enabled, serve HTTPS with automatic certificates
    #[cfg(feature = "acme")]
    {
        if let Some(domain) = cfg.acme_domain.clone() {
            return serve_with_acme(
                app,
                domain,
                cfg.acme_contact_email.clone(),
                cfg.acme_cache_dir.clone(),
            )
            .await;
        }
    }

    // Fallback: plain HTTP
    let listener = tokio::net::TcpListener::bind(cfg.bind_addr).await?;
    info!("listening on {}", cfg.bind_addr);
    axum::serve(listener, app).await?;
    Ok(())
}

fn load_config() -> anyhow::Result<AppConfig> {
    let bind_host = std::env::var("BIND_HOST").unwrap_or_else(|_| "0.0.0.0".into());
    let bind_port: u16 = std::env::var("BIND_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8080);
    let ssh_host =
        std::env::var("SSH_HOST").unwrap_or_else(|_| "x220-nixos.tail19d694.ts.net".into());
    let ssh_user = std::env::var("SSH_USER").ok();
    let wled_host = std::env::var("WLED_HOST").unwrap_or_else(|_| "127.0.0.1".into()); // host as seen from SSH host
    let wled_port: u16 = std::env::var("WLED_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(80);
    let local_tunnel_port: u16 = std::env::var("LOCAL_TUNNEL_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(18080);
    let text_param_key = std::env::var("TEXT_PARAM_KEY").ok(); // e.g. "TT" if a Text usermod is installed
    let text_preset_id = std::env::var("TEXT_PRESET_ID")
        .ok()
        .and_then(|s| s.parse().ok());
    // ACME options
    #[cfg(feature = "acme")]
    let acme_domain = std::env::var("ACME_DOMAIN").ok();
    #[cfg(feature = "acme")]
    let acme_contact_email = std::env::var("ACME_CONTACT_EMAIL").ok();
    #[cfg(feature = "acme")]
    let acme_cache_dir = std::env::var("ACME_CACHE_DIR").unwrap_or_else(|_| "./acme-cache".into());

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
        #[cfg(feature = "acme")]
        acme_domain,
        #[cfg(feature = "acme")]
        acme_contact_email,
        #[cfg(feature = "acme")]
        acme_cache_dir,
    })
}

async fn index(State(_state): State<AppState>) -> impl IntoResponse {
    let mut html = String::with_capacity(4096);
    html.push_str("<!-- UI_VERSION: wheel-v4 -->\n");
    html.push_str(INDEX_HTML_HEADER);
    html.push_str(INDEX_HTML_FOOTER);
    (
        [
            (header::CACHE_CONTROL, "no-store, max-age=0"),
            (header::PRAGMA, "no-cache"),
        ],
        Html(html),
    )
}

static APP_JS: &str = r#"(function(){
  const I18N = {
    nl: {
      subtitle: 'Laat je felicitatie schitteren op het LED gordijn ✨',
      message_label: 'Jouw bericht',
      color_label: 'Kleur',
      submit_btn: 'Stuur naar gordijn',
      note: 'Max 64 tekens. Houd het lief en feestelijk 💛',
      queue_title: 'Berichten wachtrij',
      queue_empty: 'Geen berichten in de wachtrij…',
      footer: 'Met liefde gemaakt • Wens fijn en respectvol 💐',
      error_prefix: 'Mislukt:',
      placeholder_text: 'Liefde, geluk en een lang leven samen!'
    },
    fr: {
      subtitle: 'Faites briller votre félicitation sur le rideau LED ✨',
      message_label: 'Votre message',
      color_label: 'Couleur',
      submit_btn: 'Envoyer au rideau',
      note: '64 caractères max. Restez gentil et festif 💛',
      queue_title: 'File d’attente des messages',
      queue_empty: 'Aucun message dans la file d’attente…',
      footer: 'Fait avec amour • Souhaitez avec gentillesse 💐',
      error_prefix: 'Échec :',
      placeholder_text: 'Amour, bonheur et une longue vie ensemble !'
    },
    de: {
      subtitle: 'Lass deine Glückwünsche auf dem LED‑Vorhang erstrahlen ✨',
      message_label: 'Deine Nachricht',
      color_label: 'Farbe',
      submit_btn: 'An den Vorhang senden',
      note: 'Max. 64 Zeichen. Bitte lieb und festlich 💛',
      queue_title: 'Nachrichten‑Warteschlange',
      queue_empty: 'Keine Nachrichten in der Warteschlange…',
      footer: 'Mit Liebe gemacht • Wünsche freundlich und respektvoll 💐',
      error_prefix: 'Fehlgeschlagen:',
      placeholder_text: 'Liebe, Glück und ein langes gemeinsames Leben!'
    }
  };
  function getLang(){ return localStorage.getItem('lang') || 'nl'; }
  function setLang(l){ localStorage.setItem('lang', l); applyTranslations(); markActiveLang(); refreshQueue().catch(()=>{}); }
  function tr(key){ const lang = getLang(); return (I18N[lang] && I18N[lang][key]) || (I18N['nl'] && I18N['nl'][key]) || null; }
  function applyTranslations(){
    document.querySelectorAll('[data-i18n]').forEach(el=>{
      const k = el.getAttribute('data-i18n'); if(!k) return;
      const v = tr(k);
      if(v != null) el.textContent = v; // only replace when we have a translation
    });
    const input = document.getElementById('text'); if(input){ const ph = tr('placeholder_text'); if(ph!=null) input.placeholder = ph; }
  }
  function markActiveLang(){ const sel = document.getElementById('langSelector'); if(!sel) return; const cur = getLang(); sel.querySelectorAll('button[data-lang]').forEach(btn=>{ btn.classList.toggle('active', btn.getAttribute('data-lang')===cur); }); }

  async function submitMessage(ev){
    ev.preventDefault();
    const fd = new FormData(ev.target);
    const res = await fetch('/api/message', { method: 'POST', body: new URLSearchParams(fd) });
    if(res.ok){ ev.target.reset(); try{ await refreshQueue(); }catch(e){} }
    else { const tt = await res.text(); const pref = tr('error_prefix') || 'Mislukt:'; alert(pref+' '+tt); }
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
      const li = document.createElement('li'); li.className='queue-empty'; li.textContent= tr('queue_empty') || 'Geen berichten in de wachtrij…'; ul.appendChild(li); return;
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
    // lang selector
    const sel = document.getElementById('langSelector');
    if(sel){ sel.addEventListener('click', (e)=>{ const btn = e.target.closest('button[data-lang]'); if(btn){ setLang(btn.getAttribute('data-lang')); }}); }
    applyTranslations();
    markActiveLang();
    refreshQueue();
    setInterval(()=>{ refreshQueue().catch(()=>{}); }, 5000);
    setInterval(tickTimer, 1000);
  }

  if (document.readyState !== 'loading') boot(); else window.addEventListener('DOMContentLoaded', boot);
})();"#;

async fn app_js() -> impl IntoResponse {
    (
        [
            (
                header::CONTENT_TYPE,
                "application/javascript; charset=utf-8",
            ),
            (header::CACHE_CONTROL, "no-store, max-age=0"),
            (header::PRAGMA, "no-cache"),
        ],
        APP_JS,
    )
}

#[derive(Deserialize, Debug)]
struct MessageForm {
    text: String,
    color: Option<String>, // #rrggbb
}

async fn send_message(
    State(state): State<AppState>,
    Form(form): Form<MessageForm>,
) -> impl IntoResponse {
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
                let new_disp = CurrentDisplay {
                    id,
                    text: text.clone(),
                    color: form.color.clone(),
                    started: Instant::now(),
                };
                *cur = Some(new_disp.clone());
                switched_now = true;
                drop(cur);
                if let Err(e) =
                    apply_display(&state, &new_disp.text, new_disp.color.as_deref()).await
                {
                    error!(?e, "apply_display failed");
                }
            }
        }
    }

    if !switched_now {
        // Enqueue for rotation
        let mut q = state.queue.lock().await;
        q.push_back(QueuedMessage {
            id,
            text,
            color: form.color,
        });
    }
    (
        StatusCode::OK,
        if switched_now { "switched" } else { "queued" },
    )
        .into_response()
}

// Upload endpoint removed

fn parse_hex_color(s: &str) -> Option<(u8, u8, u8)> {
    let s = s.strip_prefix('#').unwrap_or(s);
    if s.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some((r, g, b))
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
    if let Some(user) = &state.cfg.ssh_user {
        target.push_str(user);
        target.push('@');
    }
    target.push_str(&state.cfg.ssh_host);

    let forward = format!(
        "127.0.0.1:{}:{}:{}",
        state.cfg.local_tunnel_port, state.cfg.wled_host, state.cfg.wled_port
    );

    info!("starting ssh tunnel to {} forwarding {}", target, forward);
    let mut cmd = Command::new("ssh");
    cmd.arg("-NT")
        .arg("-o")
        .arg("ExitOnForwardFailure=yes")
        .arg("-o")
        .arg("ServerAliveInterval=10")
        .arg("-o")
        .arg("ServerAliveCountMax=3")
        .arg("-L")
        .arg(forward)
        .arg(target)
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    // launch in background and give it a moment
    let _child = cmd.spawn()?; // intentionally not awaited; relies on autossh-like keepalive via supervise
    time::sleep(Duration::from_millis(400)).await;
    Ok(())
}

// no-op

#[cfg(feature = "acme")]
async fn serve_with_acme(
    app: Router,
    domain: String,
    contact: Option<String>,
    cache_dir: String,
) -> anyhow::Result<()> {
    use axum::{http::Uri, response::IntoResponse};
    use axum_server::Handle;
    use rustls_acme::{caches::DirCache, AcmeConfig};

    // Configure Let's Encrypt (use TLS-ALPN-01 on 443 and redirect 80->443)
    let mut cfg = AcmeConfig::new(vec![domain.clone()])
        .cache(DirCache::new(cache_dir))
        .directory_lets_encrypt(true)
        .ocsp(true);
    if let Some(c) = contact {
        cfg = cfg.contact_push(format!("mailto:{}", c));
    }

    let mut state = cfg.state();
    tokio::spawn(async move {
        loop {
            if let Err(e) = state.execute().await {
                error!(?e, "acme state error");
                time::sleep(Duration::from_secs(5)).await;
            }
        }
    });

    let acceptor = rustls_acme::axum::AxumAcceptor::new(state);
    let handle = Handle::new();

    // HTTPS server
    let https = axum_server::bind_acceptor((std::net::Ipv4Addr::UNSPECIFIED, 443).into(), acceptor)
        .handle(handle.clone())
        .serve(app.into_make_service());

    // HTTP redirect server
    let redir_app = Router::new().fallback(move |uri: Uri| {
        let host = domain.clone();
        async move {
            let location = format!("https://{}{}", host, uri);
            (
                StatusCode::MOVED_PERMANENTLY,
                [(header::LOCATION, location)],
            )
                .into_response()
        }
    });
    let http = axum_server::bind((std::net::Ipv4Addr::UNSPECIFIED, 80).into())
        .handle(handle.clone())
        .serve(redir_app.into_make_service());

    info!("ACME enabled; serving HTTPS for domain");
    tokio::select! {
        r = https => { r?; },
        r = http => { r?; },
    }
    Ok(())
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
                    if let Err(e) =
                        apply_display(&state, &display.text, display.color.as_deref()).await
                    {
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
                if let Err(e) = apply_display(&state, &d.text, d.color.as_deref()).await {
                    error!(?e, "apply_display failed");
                }
            }
        }
        time::sleep(Duration::from_millis(900)).await;
    }
}

async fn apply_display(state: &AppState, text: &str, color: Option<&str>) -> anyhow::Result<()> {
    if let Err(e) = ensure_tunnel(state).await {
        error!(?e, "tunnel ensure failed");
    }
    let base = format!("http://127.0.0.1:{}", state.cfg.local_tunnel_port);

    // Ensure scrolling text effect is active first
    // If a preset is provided, switch to it (assumed to be the scrolling text preset).
    // Otherwise, pick the scrolling text effect index and include it in the next state update.
    let mut fx_idx: Option<usize> = None;
    if let Some(ps) = state.cfg.text_preset_id {
        let _ = state
            .client
            .post(format!("{}/json/state", base))
            .json(&serde_json::json!({"ps": ps}))
            .send()
            .await;
    } else {
        fx_idx = find_text_effect_index(&state.client, &base).await;
    }

    // Now apply color (as Color 1), select a palette that respects Color 1, and set the segment name to the message.
    // If effect index is known (no preset), set it alongside to ensure the effect is scrolling text.
    let (r, g, b) = color.and_then(parse_hex_color).unwrap_or((255, 215, 0));
    let bri: u8 = 128;
    // Force the effect's color mode to use Color 1 and set font size to max.
    // WLED 0.14+: preferred is the consolidated options array `o: [o1, o2, ...]`.
    // For Scrolling Text: o1 = color mode (0 = Color 1), o2 = font size (max 255).
    // Keep legacy fields (o1/o2/c1) for compatibility.
    let mut seg = serde_json::json!({
        "id": 0,
        "n": text,
        "col": [[r, g, b]],
        "o": [0, 255],
        "o1": 0,
        "c2": 255,
        "c1": 0
    });
    if let Some(p) = find_color1_palette_index(&state.client, &base).await { seg["pal"] = serde_json::json!(p); }
    if let Some(idx) = fx_idx {
        seg["fx"] = serde_json::json!(idx);
    }
    let json_body = serde_json::json!({ "on": true, "bri": bri, "seg": [ seg ] });
    let _ = state
        .client
        .post(format!("{}/json/state", base))
        .json(&json_body)
        .send()
        .await;

    // Optional legacy text API
    if let Some(key) = &state.cfg.text_param_key {
        let url = format!("{}/win?{}={}", base, key, urlencoding::encode(text));
        let _ = state.client.get(url).send().await;
    }
    Ok(())
}

static TEXT_EFFECT_INDEX: once_cell::sync::OnceCell<usize> = once_cell::sync::OnceCell::new();
async fn find_text_effect_index(client: &reqwest::Client, base: &str) -> Option<usize> {
    if let Some(idx) = TEXT_EFFECT_INDEX.get() {
        return Some(*idx);
    }
    let url = format!("{}/json/effects", base);
    let res = client.get(url).send().await.ok()?;
    let effects: serde_json::Value = res.json().await.ok()?;
    let arr = effects.as_array()?;
    let mut candidate: Option<usize> = None;
    for (i, v) in arr.iter().enumerate() {
        if let Some(name) = v.as_str() {
            let lc = name.to_lowercase();
            if lc.contains("scroll") && lc.contains("text") {
                candidate = Some(i);
                break;
            }
            if candidate.is_none() && lc.contains("text") {
                candidate = Some(i);
            }
        }
    }
    if let Some(i) = candidate {
        let _ = TEXT_EFFECT_INDEX.set(i);
        return Some(i);
    }
    None
}

static COLOR1_PALETTE_INDEX: once_cell::sync::OnceCell<usize> = once_cell::sync::OnceCell::new();
async fn find_color1_palette_index(client: &reqwest::Client, base: &str) -> Option<usize> {
    if let Some(idx) = COLOR1_PALETTE_INDEX.get() { return Some(*idx); }
    let url = format!("{}/json/palettes", base);
    let res = client.get(url).send().await.ok()?;
    let palettes: serde_json::Value = res.json().await.ok()?;
    let arr = palettes.as_array()?;
    let mut candidate: Option<usize> = None;
    for (i, v) in arr.iter().enumerate() {
        if let Some(name) = v.as_str() {
            let lc = name.to_lowercase();
            if lc.contains("primary") || lc.contains("color 1") || lc.contains("single") || lc.contains("solid") {
                candidate = Some(i);
                break;
            }
        }
    }
    if let Some(i) = candidate { let _ = COLOR1_PALETTE_INDEX.set(i); return Some(i); }
    None
}

async fn get_queue(State(state): State<AppState>) -> impl IntoResponse {
    let cur = state.current.lock().await;
    let (current, elapsed) = if let Some(ref c) = *cur {
        (
            Some(serde_json::json!({
                "id": c.id,
                "text": c.text,
                "color": c.color,
            })),
            c.started.elapsed().as_secs(),
        )
    } else {
        (None, 0)
    };
    drop(cur);
    let q = state.queue.lock().await;
    let items: Vec<_> = q
        .iter()
        .map(|m| {
            serde_json::json!({
                "id": m.id,
                "text": m.text,
                "color": m.color,
            })
        })
        .collect();
    let body = serde_json::json!({
        "current": current,
        "elapsed_seconds": elapsed,
        "items": items,
    });
    (
        [
            (header::CACHE_CONTROL, "no-store, max-age=0"),
            (header::PRAGMA, "no-cache"),
            (header::CONTENT_TYPE, "application/json"),
        ],
        body.to_string(),
    )
}

async fn admin_page() -> impl IntoResponse {
    let html = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Trouw Gordijn • Admin</title>
  <style>
    body { background:#101014; color:#faf8f5; font-family: system-ui, -apple-system, Segoe UI, Roboto, Ubuntu, Cantarell, Noto Sans, Helvetica, Arial; margin:0; padding:24px; }
    .card { background:#161823; border:1px solid #2a2d3a; border-radius:16px; padding:18px; max-width:860px; margin:0 auto; }
    h1 { margin:0 0 10px; }
    .row { display:flex; gap:10px; align-items:center; }
    button { background:#2a2d3a; color:#fff; border:none; padding:8px 12px; border-radius:8px; cursor:pointer; }
    button.danger { background:#7b1c1c; }
    ul { list-style:none; padding:0; margin:0; display:flex; flex-direction:column; gap:8px; }
    li { display:flex; gap:10px; align-items:center; padding:10px; border-radius:10px; background:#1d1f2a; border:1px solid #2a2d3a; }
    .swatch { width:14px; height:14px; border-radius:3px; border:1px solid rgba(0,0,0,0.25); }
    .text { flex:1; white-space:nowrap; overflow:hidden; text-overflow:ellipsis; }
    .tag { font-size:12px; opacity:.8; padding:2px 6px; border-radius:999px; background:#2a2d3a; }
  </style>
</head>
<body>
  <div class="card">
    <h1>Admin • Queue</h1>
    <div class="row" style="margin-bottom:10px">
      <button id="refresh">Refresh</button>
    </div>
    <ul id="list"><li>Loading…</li></ul>
  </div>
  <script>
  async function fetchQueue(){ const r = await fetch('/api/queue', {cache:'no-store'}); return await r.json(); }
  async function removeItem(id){
    const res = await fetch('/api/admin/remove', { method:'POST', headers:{'Content-Type':'application/x-www-form-urlencoded'}, body: new URLSearchParams({ id:String(id) }) });
    if(!res.ok){ alert('Remove failed: '+await res.text()); }
    await render();
  }
  function li(item, label){
    const li = document.createElement('li');
    const sw = document.createElement('span'); sw.className='swatch'; sw.style.background = item.color || '#ffd700'; li.appendChild(sw);
    const t = document.createElement('span'); t.className='text'; t.textContent = item.text; li.appendChild(t);
    if(label){ const tag = document.createElement('span'); tag.className='tag'; tag.textContent = label; li.appendChild(tag); }
    const btn = document.createElement('button'); btn.className='danger'; btn.textContent='Remove'; btn.onclick = ()=> removeItem(item.id); li.appendChild(btn);
    return li;
  }
  async function render(){
    const data = await fetchQueue();
    const ul = document.getElementById('list'); ul.innerHTML='';
    if(data.current){ ul.appendChild(li(data.current, 'Current')); }
    for(const it of data.items||[]){ ul.appendChild(li(it)); }
    if(!data.current && (!data.items || data.items.length===0)){
      const e = document.createElement('li'); e.textContent = 'Queue is empty'; ul.appendChild(e);
    }
  }
  document.getElementById('refresh').onclick = render;
  render();
  </script>
</body>
</html>
"#;
    (
        [
            (header::CACHE_CONTROL, "no-store, max-age=0"),
            (header::PRAGMA, "no-cache"),
        ],
        Html(html.to_string()),
    )
}

#[derive(Deserialize)]
struct RemoveForm {
    id: u64,
}

async fn admin_remove(
    State(state): State<AppState>,
    Form(f): Form<RemoveForm>,
) -> impl IntoResponse {
    // Remove from queue
    {
        let mut q = state.queue.lock().await;
        q.retain(|m| m.id != f.id);
    }
    // If removing current, clear it
    {
        let mut cur = state.current.lock().await;
        if let Some(c) = cur.as_ref() {
            if c.id == f.id {
                *cur = None;
            }
        }
    }
    (StatusCode::OK, "ok")
}
