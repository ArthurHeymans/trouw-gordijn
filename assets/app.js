(function(){
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
})();

