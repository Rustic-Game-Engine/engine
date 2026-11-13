// Render every shipped preset through the real exported player and preserve evidence.
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {createServer} from 'node:http';
import {mkdir,mkdtemp,readFile,rm,writeFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {dirname,extname,join,resolve,sep} from 'node:path';
import {fileURLToPath} from 'node:url';
import {chromium} from 'playwright';
const workspace=resolve(dirname(fileURLToPath(import.meta.url)),'../..');
const temporary=await mkdtemp(join(tmpdir(),'rustic-preset-gallery-'));
const site=join(temporary,'website');
const artifact=process.env.RUSTIC_BROWSER_PRESETS_ARTIFACT_DIR;
let browser,server;
try {
 const fixture=spawnSync('cargo',['test','-p','rustic-editor','--profile',process.env.RUSTIC_BUILD_PROFILE||'dev-fast','write_browser_presets_fixture','--','--ignored'],{cwd:workspace,stdio:'inherit',env:{...process.env,CARGO_INCREMENTAL:'0',RUSTIC_BROWSER_PRESETS_DIR:temporary}});
 assert.equal(fixture.status,0,'Could not export actual preset gallery');
 const mime={'.js':'text/javascript','.html':'text/html','.json':'application/json','.wasm':'application/wasm'};
 server=createServer(async(request,response)=>{try{
  const path=new URL(request.url,'http://localhost').pathname;
  const file=resolve(site,`.${decodeURIComponent(path==='/'?'/index.html':path)}`);
  if(!file.startsWith(site+sep))throw Error('Invalid path');
  response.setHeader('Content-Type',mime[extname(file)]||'application/octet-stream');
  response.setHeader('Cache-Control','no-store');response.end(await readFile(file));
 }catch{response.writeHead(404);response.end('Not found');}});
 await new Promise(ready=>server.listen(0,'127.0.0.1',ready));
 browser=await chromium.launch({executablePath:process.env.RUSTIC_CHROMIUM||undefined,headless:true,args:['--enable-unsafe-swiftshader','--use-angle=swiftshader','--disable-dev-shm-usage','--no-sandbox']});
 const page=await browser.newPage({viewport:{width:800,height:450}});
 const logs=[],errors=[];
 page.on('console',message=>logs.push(message.text()));page.on('pageerror',error=>errors.push(error.message));
 const waitLog=async marker=>{const deadline=Date.now()+60000;while(!logs.some(line=>line.includes(marker))){
  const status=await page.locator('#status').textContent();assert.ok(!status.startsWith('Game could'),`${status}\n${logs.join('\n')}`);
  assert.ok(Date.now()<deadline,`Missing ${marker}\n${logs.join('\n')}`);await new Promise(done=>setTimeout(done,50));
 }};
 const frames=async count=>{const stamp=Number(await page.locator('#game').getAttribute('data-frame'));await page.waitForFunction(({stamp,count})=>Number(document.getElementById('game').dataset.frame)>=stamp+count,{stamp,count},{timeout:60000});};
 const tap=async key=>{await page.keyboard.down(key);await frames(2);await page.keyboard.up(key);await frames(2);};
 await page.goto(`http://127.0.0.1:${server.address().port}`);
 await waitLog('gallery-ready:35');await page.locator('#game').focus();
 assert.match(await page.locator('#gpu').evaluate(canvas=>canvas.getContext('webgl2')?.getParameter(WebGL2RenderingContext.VERSION)||''),/WebGL 2/i);
 if(artifact)await mkdir(artifact,{recursive:true});
 const results=[];
 for(let index=1;index<=35;index++){
  await tap('ArrowRight');await waitLog(`gallery-selected:${index}:`);await frames(3);
  const name=logs.find(line=>line.includes(`gallery-selected:${index}:`)).split(`gallery-selected:${index}:`)[1];
  await page.locator('#game').evaluate(canvas=>{window.__presetReference=new Uint8ClampedArray(canvas.getContext('2d').getImageData(0,0,canvas.width,canvas.height).data);});
  // Start and sample immediately, before short bursts finish.
  await page.keyboard.down('ArrowUp');await waitLog(`gallery-started:${index}`);await page.keyboard.up('ArrowUp');
  const compare=()=>page.locator('#game').evaluate(canvas=>{const data=canvas.getContext('2d').getImageData(0,0,canvas.width,canvas.height).data;let changed=0,difference=0;for(let i=0;i<data.length;i+=4){const d=Math.max(Math.abs(data[i]-window.__presetReference[i]),Math.abs(data[i+1]-window.__presetReference[i+1]),Math.abs(data[i+2]-window.__presetReference[i+2]));if(d>8)changed++;difference+=d;}return{changed,meanDifference:difference/(data.length/4)};});
  await frames(2);const early=await compare();
  const slug=`${String(index).padStart(2,'0')}-${name.toLowerCase().replace(/[^a-z0-9]+/g,'-')}`;
  if(artifact)await page.locator('#game').screenshot({path:join(artifact,`${slug}-early.png`)});
  await frames(40);const late=await compare();
  if(artifact)await page.locator('#game').screenshot({path:join(artifact,`${slug}-late.png`)});
  results.push({index,name,early,late});
  // These checks establish real rendered output, not subjective artistic approval.
  assert.ok(Math.max(early.changed,late.changed)>5,`Preset ${index} ${name} produced no visible pixels: ${JSON.stringify({early,late})}`);
  console.log(`PASS preset ${index}/35 ${name}: early=${early.changed} late=${late.changed} changed pixels`);
 }
 assert.deepEqual(errors,[],'Preset gallery raised browser errors');
 if(artifact)await writeFile(join(artifact,'presets.json'),JSON.stringify({backend:'WebGL2 CPU fallback',presets:results},null,2));
 console.log('PASS: all 35 shipped presets produced actual exported WebGL output');
}finally{
 if(browser)await browser.close();if(server)await new Promise(done=>server.close(done));
 await rm(temporary,{recursive:true,force:true});
}
