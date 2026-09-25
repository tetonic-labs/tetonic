// Usage: node validate_mermaid.mjs <temporary npm prefix containing mermaid/jsdom>
import {readFileSync, writeFileSync} from 'node:fs';
import {resolve, dirname} from 'node:path';
import {pathToFileURL, fileURLToPath} from 'node:url';
const here=dirname(fileURLToPath(import.meta.url));
const prefix=process.argv[2];
if(!prefix) throw new Error('Provide temporary npm prefix');
const {JSDOM}=await import(pathToFileURL(resolve(prefix,'node_modules/jsdom/lib/api.js')));
const dom=new JSDOM('<!doctype html><html><body></body></html>');
globalThis.window=dom.window;
globalThis.document=dom.window.document;
const {default:mermaid}=await import(pathToFileURL(resolve(prefix,'node_modules/mermaid/dist/mermaid.esm.mjs')));
mermaid.initialize({startOnLoad:false,securityLevel:'strict'});
const diagrams=JSON.parse(readFileSync(resolve(here,'diagrams.json'),'utf8'));
const results=[];
for(const d of diagrams){
  try { await mermaid.parse(d.source);results.push({document:d.document,number:d.number,passed:true}); }
  catch(e){results.push({document:d.document,number:d.number,passed:false,error:String(e)});}
}
const output={mermaid_version:'11.12.0',jsdom_version:'26.1.0',diagrams:results.length,passed:results.every(r=>r.passed),results};
writeFileSync(resolve(here,'mermaid-results.json'),JSON.stringify(output,null,2)+'\n');
console.log(JSON.stringify({diagrams:output.diagrams,passed:output.passed,failures:results.filter(r=>!r.passed)},null,2));
dom.window.close();
process.exitCode=output.passed?0:1;
