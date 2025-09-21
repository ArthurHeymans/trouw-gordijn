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
document.addEventListener('DOMContentLoaded', () => {
  document.getElementById('refresh').onclick = render;
  render();
});

