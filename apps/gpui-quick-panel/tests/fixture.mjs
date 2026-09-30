// Synthetic daemon for repeatable UI checks. Never reads real user history.
import { createServer } from 'node:http';
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';

const rows = Array.from({ length: 125 }, (_, index) => ({
  entryId: `fixture-${index}`,
  contentType: 'text',
  activeTimeMs: Date.now() - index * 60000,
  tags: index === 0 ? ['favorited', '工作'] : [],
  textPreview: index === 0 ? 'GPUI quick panel paste verification · 工作 设计' : index === 1 ? '中文搜索验证：剪贴板历史' : `Clipboard sample ${index} - a long preview that remains on one line even when the window is narrow`,
  charCount: 80,
  mimeType: 'text/plain',
  fileExtensions: [], fileNames: [], filePaths: [], linkUrls: [], sourceDevice: null,
  payloadState: index === 2 ? 'Lost' : null,
}));
const state = { searches: 0, restores: [], deleted: [], authenticated: 0, lastSearch: null, requests: [], searchStarts: [], settingsReads: 0, tagsReads: 0 };
const fullText = new Map([
  ['fixture-4', Array.from({length:120}, (_, i) => `第 ${i+1} 行：这是一段用于验证预览高度上限与内部滚动的长文本。`).join('\n')],
  ['fixture-5', '多行内容验证\n第二行：预览跟随记录\n第三行：小箭头保持对齐\n第四行：窗口高度由内容决定\n第五行：历史窗口保持不动'],
]);
rows[4].textPreview = '长文本验证（120 行）';
rows[5].textPreview = '多行内容验证（5 行）';
const imageBytes=new Map();
const settings=JSON.parse(readFileSync(new URL('./settings.json',import.meta.url),'utf8'));
settings.general.theme=process.env.UC_GPUI_FIXTURE_THEME??'system';
if(process.env.UC_GPUI_IMAGE_FIXTURES==='1'){
  const images=[['landscape','横图预览 · 设计'],['portrait','竖图预览'],['transparent','透明图片预览'],['small','小图预览']];
  const imageRows=images.map(([name,label])=>{
    const entryId=`image-${name}`;
    imageBytes.set(entryId,readFileSync(new URL(`./images/${name}.png`,import.meta.url)));
    return {...rows[0],entryId,textPreview:label,contentType:'image',mimeType:'image/png',tags:['image', ...(name==='landscape'?['favorited','工作']:name==='portrait'?['工作']:name==='transparent'?['favorited']:[])],payloadState:null,charCount:null};
  });
  if(process.env.UC_GPUI_FILTER_FIXTURES==='1') imageRows.reverse();
  rows.unshift(...imageRows);
}
const server = createServer(async (request, response) => {
  const url = new URL(request.url, 'http://localhost');
  const json = (status, body) => {
    response.writeHead(status, { 'content-type': 'application/json' });
    response.end(JSON.stringify(body));
  };
  if (url.pathname === '/__test/state') return json(200, state);
  if (url.pathname === '/auth/connect') {
    if (request.headers.authorization !== 'Bearer fixture-token') return json(401, {});
    state.authenticated++;
    return json(200, { data: { sessionToken: 'fixture-session', expiresInSecs: 3600, refreshAtSecs: 3000 }, ts: Date.now() });
  }
  if (request.headers.authorization !== 'Session fixture-session') return json(401, {});
  if (url.pathname === '/settings') { state.settingsReads++; return json(200,{data:settings,ts:Date.now()}); }
  if (url.pathname === '/search/tags') {
    state.tagsReads++;
    const builtins = ['link', 'code', 'favorited', 'image', 'directory'];
    return json(200, { data: [...builtins, '工作', '设计素材与灵感收集', '项目资料归档', '稍后处理', '待整理', '参考资料', '生活', '阅读', '研究', '截图', '临时备忘'].map(tagId => ({
      tagId, count: rows.filter(row => row.tags.includes(tagId)).length, isBuiltin: builtins.includes(tagId),
    })), ts: Date.now() });
  }
  if (url.pathname === '/paired-devices') return json(200, {data:[],ts:Date.now()});
  if (url.pathname === '/search/query') {
    state.searches++;
    const query = url.searchParams.get('query') ?? '';
    state.searchStarts.push(query);
    if (query === 'error') return json(503, { error: { code: 'index_rebuilding', message: 'Synthetic failure' } });
    if (query === 'slow') await new Promise(resolve => setTimeout(resolve, 900));
    const type=url.searchParams.get('contentTypes'); const tags=(url.searchParams.get('tags') ?? '').split(',').filter(Boolean);
    const offset=Number(url.searchParams.get('offset')??0), limit=Number(url.searchParams.get('limit')??50);
    state.lastSearch={query, contentType:type, tags, offset, limit};
    state.requests.push(state.lastSearch);
    const results = rows.filter(row => row.textPreview.toLowerCase().includes(query.toLowerCase())&&(!type||row.contentType===type)&&(!tags.length||tags.some(tag=>row.tags.includes(tag))));
    return json(200, { data: { items: results.slice(offset, offset + limit), total: results.length, hasMore: results.length > offset + limit, state: 'ready' }, ts: Date.now() });
  }
  if(request.method==='GET'&&url.pathname.startsWith('/clipboard/entries/')){
    if(url.pathname.endsWith('/resource')){
      const bytes=imageBytes.get(url.pathname.split('/').at(-2));
      if(!bytes)return json(404,{});
      return json(200,{data:{blobId:null,mimeType:'image/png',sizeBytes:bytes.length,url:null,inlineData:bytes.toString('base64')},ts:Date.now()});
    }
    const row=rows.find(row=>row.entryId===url.pathname.split('/').at(-1));
    if(!row)return json(404,{});
    const content=fullText.get(row.entryId)??row.textPreview;
    return json(200,{data:{id:row.entryId,content,sizeBytes:Buffer.byteLength(content),createdAtMs:row.activeTimeMs,activeTimeMs:row.activeTimeMs,mimeType:row.mimeType},ts:Date.now()});
  }
  if (request.method === 'POST' && url.pathname.startsWith('/clipboard/restore/')) {
    const row = rows.find(row => row.entryId === url.pathname.split('/').at(-1));
    if (!row || row.payloadState === 'Lost') return json(404, {});
    const result = spawnSync('pbcopy', { input: row.textPreview });
    if (result.status !== 0) return json(500, {});
    state.restores.push(row.entryId);
    response.writeHead(204);
    return response.end();
  }
  if (request.method === 'DELETE' && url.pathname.startsWith('/clipboard/entries/')) {
    // Records the request only; the rows stay so that every test starts from the same list.
    state.deleted.push(url.pathname.split('/').at(-1));
    response.writeHead(204);
    return response.end();
  }
  json(404, {});
});
server.listen(Number(process.env.UC_GPUI_FIXTURE_PORT ?? 48173), '127.0.0.1', () => console.log(`Synthetic daemon listening on http://127.0.0.1:${server.address().port}`));
