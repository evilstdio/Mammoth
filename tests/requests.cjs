// Local-only HTTP integration tests for mammoth.requests. Run: node tests/requests.cjs
// Every scenario runs as a single file, a project JAR and a packaged JAR, all
// under -Xverify:all, against a server on 127.0.0.1. Nothing else is contacted.
const fs = require('fs'), path = require('path'), os = require('os'), http = require('http');
const assert = require('assert/strict'), { spawn } = require('child_process');
const compiler = process.env.MAMMOTH || path.resolve(__dirname, '../target/release/mammoth.exe');
const folder = fs.mkdtempSync(path.join(os.tmpdir(), 'mammoth-requests-'));
const only = process.env.ONLY;
const JAVA = ['-Xverify:all', '-Dsun.stdout.encoding=UTF-8', '-Dfile.encoding=UTF-8'];
const seen = [];

const routes = {
  '/get': (req, res) => { res.writeHead(200, { 'X-Test': 'yes', 'Content-Type': 'text/plain; charset=utf-8' }); res.end('hello'); },
  '/post': (req, res) => { res.writeHead(201, { 'X-Test': 'yes' }); res.end('hello'); },
  '/error': (req, res) => { res.writeHead(422); res.end('rejected'); },
  '/redirect': (req, res) => { res.writeHead(302, { Location: '/get' }); res.end(); },
  '/slow': (req, res) => { setTimeout(() => { res.writeHead(200); res.end('late'); }, 1400); },
  '/json': (req, res) => {
    res.writeHead(200, { 'Content-Type': 'application/json' });
    res.end('{"name":"m\\u0041\u{1F9A3}","big":9007199254740993,"list":[1,2.5,true,null,{"x":"y"}],"empty":{}}');
  },
  '/badjson': (req, res) => { res.writeHead(200); res.end('{"a": '); },
  '/deepjson': (req, res) => { res.writeHead(200); res.end('['.repeat(200000)); },
};

const server = http.createServer((req, res) => {
  const chunks = [];
  req.on('data', x => chunks.push(x));
  req.on('end', () => {
    const raw = Buffer.concat(chunks);
    const url = new URL(req.url, 'http://x');
    seen.push({ url: req.url, path: url.pathname, query: url.search, method: req.method, headers: req.headers, raw, body: raw.toString('utf8'), type: req.headers['content-type'] });
    (routes[url.pathname] || routes['/get'])(req, res, url, raw);
  });
});

function run(exe, args) {
  return new Promise((resolve, reject) => {
    let out = '', err = '';
    const p = spawn(exe, args);
    const timer = setTimeout(() => { p.kill(); reject(Error('Timed out: ' + args.join(' '))); }, 60000);
    p.stdout.on('data', x => out += x);
    p.stderr.on('data', x => err += x);
    p.on('error', reject);
    p.on('close', code => { clearTimeout(timer); resolve({ code, out, err }); });
  });
}

/** Writes a JavaScript string as a Mammoth text literal. */
function text(value) {
  return '"' + value.replace(/\\/g, '\\\\').replace(/"/g, '\\"').replace(/\{/g, '\\{').replace(/\}/g, '\\}')
    .replace(/\n/g, '\\n').replace(/\r/g, '\\r').replace(/\t/g, '\\t') + '"';
}

const IMPORTS = ['requests', 'Response', 'RequestError', 'Json', 'JsonError'].map(name => 'import mammoth.' + name);

let count = 0;
const modes = {
  async single(source, dir) {
    const file = path.join(dir, 'main.mam'), out = path.join(dir, 'classes');
    fs.writeFileSync(file, source);
    const built = await run(compiler, [file, out]);
    assert.equal(built.code, 0, 'single-file build: ' + built.out + built.err);
    return run('java', [...JAVA, '-cp', out, 'Main']);
  },
  async project(source, dir, extra) {
    const project = path.join(dir, 'project'), jar = path.join(dir, 'project.jar');
    fs.mkdirSync(project);
    fs.writeFileSync(path.join(project, 'main.mam'), source);
    for (const [name, content] of Object.entries(extra || {})) {
      fs.mkdirSync(path.dirname(path.join(project, name)), { recursive: true });
      fs.writeFileSync(path.join(project, name), content);
    }
    const built = await run(compiler, ['--project', project, jar]);
    assert.equal(built.code, 0, 'project build: ' + built.out + built.err);
    return run('java', [...JAVA, '-jar', jar]);
  },
  async jar(source, dir) {
    const file = path.join(dir, 'packaged.mam'), jar = path.join(dir, 'app.jar');
    fs.writeFileSync(file, source);
    const built = await run(compiler, ['--jar', file, jar]);
    assert.equal(built.code, 0, 'jar build: ' + built.out + built.err);
    return run('java', [...JAVA, '-jar', jar]);
  },
};

async function scenario(name, lines, expected, check, options = {}) {
  if (only && !name.includes(only)) return;
  const source = (options.bare ? lines : IMPORTS.concat(lines)).join('\n') + '\n';
  for (const mode of options.modes || Object.keys(modes)) {
    const dir = path.join(folder, String(count++));
    fs.mkdirSync(dir);
    seen.length = 0;
    const ran = await modes[mode](source, dir, options.files);
    assert.equal(ran.code, 0, `${name} [${mode}] exited ${ran.code}: ${ran.err}\n${ran.out}`);
    assert.equal(ran.out.replace(/\r/g, '').trimEnd(), expected.join('\n'), `${name} [${mode}]\n${ran.err}`);
    if (check) check(seen, mode);
  }
  console.log('PASS: ' + name);
}

/** Expects the compiler to reject a program with this message. */
async function rejected(name, lines, message) {
  if (only && !name.includes(only)) return;
  const dir = path.join(folder, String(count++));
  fs.mkdirSync(dir);
  const file = path.join(dir, 'main.mam');
  fs.writeFileSync(file, IMPORTS.concat(lines).join('\n') + '\n');
  const built = await run(compiler, [file, path.join(dir, 'classes')]);
  assert.notEqual(built.code, 0, name + ' should not compile');
  assert.ok((built.out + built.err).includes(message), name + ': ' + built.out + built.err);
  console.log('PASS: ' + name);
}

const catching = (expression, error, label) => `try { ${expression} } catch (error: ${error}) { print(${text(label)}) }`;

async function main(base) {
  const message = 'hello "Mammoth"!\\\n\u{1F9A3}{brace}';

  await scenario('basic calls', [
    `let a = requests.get(${text(base + '/get')})`, 'print(a.status)', 'print(a.text)', 'print(a.ok)', 'a.raiseForStatus()',
    `let b = requests.post(${text(base + '/post')}, json = {"content": ${text(message)}})`, 'print(b.status)',
    `let c = requests.get(${text(base + '/error')})`, 'print(c.status)', 'print(c.ok)',
    catching('c.raiseForStatus()', 'RequestError', 'status error'),
    `let redirect = requests.get(${text(base + '/redirect')})`, 'print(redirect.status)',
    catching(`requests.get(${text(base + '/slow')}, timeout = 1)`, 'RequestError', 'timeout error'),
    catching(`requests.get(${text(base + '/get')}, timeout = 0)`, 'RequestError', 'invalid timeout'),
    `print(requests.get(${text(base + '/get')}, 5).status)`,
  ], ['200', 'hello', 'true', '201', '422', 'false', 'status error', '302', 'timeout error', 'invalid timeout', '200'], seen => {
    assert.equal(seen.length, 6);
    assert.equal(seen[1].method, 'POST');
    assert.match(seen[1].type, /application\/json/);
    assert.deepEqual(JSON.parse(seen[1].body), { content: message });
  });

  await scenario('json request bodies', [
    `requests.post(${text(base + '/post')}, json = {"t": Json.of(${text(message)}), "i": Json.of(7), "l": Json.of(9007199254740993L), "d": Json.of(1.5), "yes": Json.of(true), "no": Json.NULL, "list": Json.of(listOf("a", "b")), "array": Json.of([1, 2]), "nested": Json.of({"x": {"y": [1.25]}})})`,
    `requests.post(${text(base + '/post')}, json = listOf(1, 2))`,
    `requests.post(${text(base + '/post')}, json = ["a"])`,
    `requests.post(${text(base + '/post')}, json = {"count": 3})`,
    `requests.post(${text(base + '/post')}, json = "plain")`,
    `requests.post(${text(base + '/post')}, json = 12)`,
    `requests.post(${text(base + '/post')}, json = Json.NULL)`,
    'let mixed: MutableMap<Text, Any> = mutableMapOf()', 'mixed["n"] = 1', 'mixed["t"] = "x"', 'mixed["b"] = false', 'mixed["inner"] = listOf(mapOf("k" to 1))',
    `requests.post(${text(base + '/post')}, json = mixed)`,
    'print("sent")',
  ], ['sent'], seen => {
    assert.equal(seen.length, 8);
    assert.equal(seen[0].body.includes('9007199254740993'), true);
    assert.deepEqual(JSON.parse(seen[0].body), { t: message, i: 7, l: 9007199254740993, d: 1.5, yes: true, no: null, list: ['a', 'b'], array: [1, 2], nested: { x: { y: [1.25] } } });
    assert.deepEqual(seen.slice(1).map(s => JSON.parse(s.body)), [[1, 2], ['a'], { count: 3 }, 'plain', 12, null, { n: 1, t: 'x', b: false, inner: [{ k: 1 }] }]);
  });

  await scenario('json values refused before sending', [
    catching(`requests.post(${text(base + '/post')}, json = 0.0 / 0.0)`, 'JsonError', 'nan'),
    catching(`requests.post(${text(base + '/post')}, json = 1.0 / 0.0)`, 'JsonError', 'infinity'),
    catching(`requests.post(${text(base + '/post')}, json = mapOf(1 to 2))`, 'JsonError', 'number key'),
    catching(`requests.post(${text(base + '/post')}, json = Thread.currentThread())`, 'JsonError', 'unsupported'),
    'let loop = MutableList<Any>()', 'loop.add(loop)',
    catching(`requests.post(${text(base + '/post')}, json = loop)`, 'JsonError', 'cycle'),
    'var deep: Any = 1', 'for i in 0 until 2000 { deep = listOf(deep) }',
    catching(`requests.post(${text(base + '/post')}, json = deep)`, 'JsonError', 'deep'),
    catching('Json.stringify({ 1 })', 'JsonError', 'lambda'),
    'let error = try { Json.stringify(Thread.currentThread()) } catch (e: JsonError) { e.getMessage() }', 'print(error)',
    'let wide = try { Json.stringify(mapOf("a" to Thread.currentThread())) } catch (e: RequestError) { "base type" }', 'print(wide)',
  ], ['nan', 'infinity', 'number key', 'unsupported', 'cycle', 'deep', 'lambda', 'A value of type Thread has no JSON form.', 'base type'],
    seen => assert.equal(seen.length, 0));

  await scenario('json decoding', [
    `let r = requests.get(${text(base + '/json')})`,
    'let v = r.json()', 'print(v["name"].asText().length())', 'print(v["big"].asLong())', 'print(v["list"][1].asDouble())', 'print(v["list"][2].asBool())', 'print(v["list"][3].isNull())', 'print(v["list"][4]["x"].asText())',
    'print(v["list"].size())', 'print(v.has("big"))', 'print(v.has("nope"))', 'print(v.find("nope") == null)', 'print(v["empty"].asMap().size())',
    'for item in v["list"].asList() { print(item.kind()) }',
    'for key in v.asMap().keySet() { print(key) }',
    catching('print(v["nope"])', 'JsonError', 'missing key'),
    catching('print(v["list"][99])', 'JsonError', 'missing index'),
    catching('print(v["big"].asInt())', 'JsonError', 'not int'),
    catching('print(v["name"].asInt())', 'JsonError', 'wrong type'),
    catching('print(v["list"]["x"])', 'JsonError', 'array as object'),
    catching(`print(requests.get(${text(base + '/badjson')}).json())`, 'JsonError', 'malformed'),
    catching(`print(requests.get(${text(base + '/deepjson')}).json())`, 'JsonError', 'too deep'),
  ], ['4', '9007199254740993', '2.5', 'true', 'true', 'y', '5', 'true', 'false', 'true', '0', 'a number', 'a number', 'a boolean', 'null', 'an object', 'name', 'big', 'list', 'empty', 'missing key', 'missing index', 'not int', 'wrong type', 'array as object', 'malformed', 'too deep']);

  const invalid = ['', ' ', '[1,]', '{a:1}', '{"a":1,}', '{"a" 1}', '01', '-01', '1.', '.5', '+1', '1e', '1e+', '-', '"\\x"', '"\\u12G4"', '"\\u12"', '[1', '{"a":1', 'tru', 'nul', 'True', '"a', '[1] 2', '"\t"', '"\n"', "'a'", 'NaN', 'Infinity', '[,1]', ' 1', '﻿1'];
  await scenario('json grammar', [
    `for bad in [${invalid.map(text).join(', ')}] {`,
    '    try { print("ACCEPTED " + Json.parse(bad).toString()) } catch (e: JsonError) { print(e.position >= 0) }',
    '}',
    `for good in [${['0', '-0', '1.5e3', '1E-2', '"\\/"', ' [ ] ', '{ }', 'null', 'true', 'false', '"\\b\\f\\n\\r\\t\\"\\\\"', '{"a":1,"a":2}', '"\\ud83e\\udda3"', '"\\ud83e"'].map(text).join(', ')}] {`,
    '    print(Json.parse(good).toString())',
    '}',
    'print(Json.parse("-3e2").asInt())', 'print(Json.parse("1.0").asInt())', 'print(Json.parse("1e400").toString())',
    catching('print(Json.parse("1.5").asInt())', 'JsonError', 'fraction'),
    catching('print(Json.parse("1e400").asDouble())', 'JsonError', 'too large'),
    catching('print(Json.parse("1e99999999999").asLong())', 'JsonError', 'huge exponent'),
    `print(Json.stringify(${text('tab\tnl\ncr\r"\\')}))`,
    'print(Json.stringify(\'c\'))', 'print(Json.stringify(null))', 'print(Json.stringify(MutableList<Int>()))',
    'print(Json.emptyObject())', 'print(Json.emptyArray())',
    'print(Json.stringify(Json.parse("\\"\\\\ud83e\\"")))',
  ], [...invalid.map(() => 'true'),
    '0', '-0', '1.5e3', '1E-2', '"/"', '[]', '{}', 'null', 'true', 'false', '"\\b\\f\\n\\r\\t\\"\\\\"', '{"a":2}', '"\u{1F9A3}"', '"\\ud83e"',
    '-300', '1', '1e400', 'fraction', 'too large', 'huge exponent', '"tab\\tnl\\ncr\\r\\"\\\\"', '"c"', 'null', '[]', '{}', '[]', '"\\ud83e"'],
    undefined, { modes: ['single'] });

  await rejected('mixed dictionary stays a compile error', [`requests.post(${text(base + '/post')}, json = {"a": 1, "b": "x"})`], 'Expected');

  await scenario('library names stay out of the program', [
    'import mammoth.requests',
    'class Json(let v: Int)', 'class JsonReader', 'class Response(let code: Int)', 'fun writeJsonText(): Int { return 4 }',
    'print(Json(3).v + Response(1).code + writeJsonText())',
    `print(requests.get(${text(base + '/get')}).status)`,
  ], ['8', '200'], undefined, { bare: true });

  await scenario('used from another project module', [
    'module demo.Main', 'import demo.Helper.fetchStatus', `print(fetchStatus(${text(base + '/error')}))`,
  ], ['422'], undefined, { bare: true, modes: ['project'], files: { 'src/helper.mam': 'module demo.Helper\nimport mammoth.requests\nexport fun fetchStatus(url: Text): Int { return requests.get(url).status }\n' } });
}

(async () => {
  await new Promise(r => server.listen(0, '127.0.0.1', r));
  try {
    await main('http://127.0.0.1:' + server.address().port);
    console.log('All mammoth.requests integration scenarios passed.');
  } finally {
    server.closeAllConnections();
    server.close();
    fs.rmSync(folder, { recursive: true, force: true });
  }
})().catch(e => { console.error(e); process.exitCode = 1; });
