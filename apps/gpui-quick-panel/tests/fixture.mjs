// Synthetic daemon for repeatable UI checks. Never reads real user history.
import { createServer } from 'node:http';
import { spawnSync } from 'node:child_process';

const rows = Array.from({ length: 125 }, (_, index) => ({
  entryId: `fixture-${index}`,
  contentType: 'text',
  activeTimeMs: Date.now() - index * 60000,
  tags: [],
  textPreview: index === 0 ? 'GPUI quick panel paste verification' : index === 1 ? '中文搜索验证：剪贴板历史' : `Clipboard sample ${index} - a long preview that remains on one line even when the window is narrow`,
  charCount: 80,
  mimeType: 'text/plain',
  fileExtensions: [], fileNames: [], filePaths: [], linkUrls: [], sourceDevice: null,
  payloadState: index === 2 ? 'Lost' : null,
}));
const state = { searches: 0, restores: [], authenticated: 0 };
const fullText = new Map([
  ['fixture-4', Array.from({length:120}, (_, i) => `第 ${i+1} 行：这是一段用于验证预览高度上限与内部滚动的长文本。`).join('\n')],
  ['fixture-5', '多行内容验证\n第二行：预览跟随记录\n第三行：小箭头保持对齐\n第四行：窗口高度由内容决定\n第五行：历史窗口保持不动'],
]);
rows[4].textPreview = '长文本验证（120 行）';
rows[5].textPreview = '多行内容验证（5 行）';
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
  if (url.pathname === '/search/tags') return json(200, {data: [{tagId:'link',count:2,isBuiltin:true},{tagId:'code',count:1,isBuiltin:true},{tagId:'favorited',count:1,isBuiltin:true},{tagId:'image',count:0,isBuiltin:true},{tagId:'directory',count:0,isBuiltin:true}],ts:Date.now()});
  if (url.pathname === '/paired-devices') return json(200, {data:[],ts:Date.now()});
  if (url.pathname === '/search/query') {
    state.searches++;
    const query = url.searchParams.get('query') ?? '';
    if (query === 'error') return json(503, { error: { code: 'index_rebuilding', message: 'Synthetic failure' } });
    if (query === 'slow') await new Promise(resolve => setTimeout(resolve, 900));
    const type=url.searchParams.get('contentTypes'); const tag=url.searchParams.get('tags');
    const results = rows.filter(row => row.textPreview.toLowerCase().includes(query.toLowerCase())&&(!type||row.contentType===type)&&(!tag||row.tags.includes(tag)));
    return json(200, { data: { items: results.slice(0, 50), total: results.length, hasMore: results.length > 50, state: 'ready' }, ts: Date.now() });
  }
  if(request.method==='GET'&&url.pathname.startsWith('/clipboard/entries/')){
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
  json(404, {});
});
server.listen(48173, '127.0.0.1', () => console.log('Synthetic daemon listening on http://127.0.0.1:48173'));
