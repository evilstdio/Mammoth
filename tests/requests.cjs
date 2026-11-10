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
  '/stall': (req, res) => { res.writeHead(200, { 'Content-Length': '100' }); res.write('part'); },
  '/headers': (req, res) => { res.writeHead(200, { 'X-Mixed-Case': 'one', 'Set-Cookie': ['a=1', 'b=2'] }); res.end(''); },
  '/binary': (req, res) => { res.writeHead(200, { 'Content-Type': 'application/octet-stream' }); res.end(Buffer.from([0, 200, 255, 65])); },
  '/latin1': (req, res) => { res.writeHead(200, { 'Content-Type': 'text/plain; charset="ISO-8859-1"' }); res.end(Buffer.from([0x63, 0x61, 0x66, 0xe9])); },
  '/badcharset': (req, res) => { res.writeHead(200, { 'Content-Type': 'text/plain; charset=nonsense-charset' }); res.end('caf\u00e9'); },
  '/utf8': (req, res) => { res.writeHead(200); res.end('caf\u00e9'); },
  '/server-error': (req, res) => { res.writeHead(503); res.end('{"retry":true}'); },
  '/set-cookie': (req, res) => { res.writeHead(200, { 'Set-Cookie': ['sid=abc; Path=/', 'theme=dark; Path=/get'] }); res.end(''); },
  '/foreign-cookie': (req, res) => { res.writeHead(200, { 'Set-Cookie': 'evil=1; Domain=other.example; Path=/' }); res.end(''); },
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

const IMPORTS = ['requests', 'Session', 'Auth', 'Response', 'Headers', 'RequestError', 'InvalidRequestError', 'TimeoutError', 'TransportError', 'HttpStatusError', 'Json', 'JsonError'].map(name => 'import mammoth.' + name);

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
    catching(`requests.get(${text(base + '/get')}, timeout = 0)`, 'InvalidRequestError', 'invalid timeout'),
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

  await scenario('http methods', [
    ...['get', 'post', 'put', 'patch', 'delete', 'head', 'options'].map(verb => `print(requests.${verb}(${text(base + '/get')}).status)`),
    `print(requests.head(${text(base + '/get')}).text.length())`,
    `print(requests.request("purge", ${text(base + '/get')}).status)`,
    `requests.put(${text(base + '/get')}, json = {"a": 1})`,
    `requests.patch(${text(base + '/get')}, text = "t")`,
    `requests.delete(${text(base + '/get')}, json = {"id": 2})`,
    catching(`requests.request("BAD METHOD", ${text(base + '/get')})`, 'InvalidRequestError', 'bad verb'),
  ], ['200', '200', '200', '200', '200', '200', '200', '0', '200', 'bad verb'], seen => {
    assert.deepEqual(seen.map(s => s.method), ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS', 'HEAD', 'PURGE', 'PUT', 'PATCH', 'DELETE']);
    assert.equal(seen[9].body, '{"a":1}');
    assert.equal(seen[10].body, 't');
    assert.equal(seen[11].body, '{"id":2}');
  });

  await scenario('headers', [
    `requests.get(${text(base + '/get')}, headers = {"X-One": "1", "x-two": "two words"})`,
    `requests.post(${text(base + '/get')}, json = {"a": 1}, headers = {"content-type": "application/vnd.test+json"})`,
    catching(`requests.get(${text(base + '/get')}, headers = {"X-Bad": "a\\r\\nX-Evil: 1"})`, 'InvalidRequestError', 'line break'),
    catching(`requests.get(${text(base + '/get')}, headers = {"Bad Name": "a"})`, 'InvalidRequestError', 'bad name'),
    ...['Host', 'content-length', 'Connection', 'Expect', 'UPGRADE'].map(name => catching(`requests.get(${text(base + '/get')}, headers = {${text(name)}: "x"})`, 'InvalidRequestError', 'managed ' + name)),
  ], ['line break', 'bad name', 'managed Host', 'managed content-length', 'managed Connection', 'managed Expect', 'managed UPGRADE'], seen => {
    assert.equal(seen.length, 2);
    assert.equal(seen[0].headers['x-one'], '1');
    assert.equal(seen[0].headers['x-two'], 'two words');
    assert.equal(seen[1].type, 'application/vnd.test+json');
  });

  await scenario('query parameters', [
    `requests.get(${text(base + '/q')}, params = {"a b": "c d", "amp": "1&2=3", "uni": "\u00e9\u{1F9A3}", "plus": "a+b"})`,
    `requests.get(${text(base + '/q?x=1')}, params = {"y": 2})`,
    `requests.get(${text(base + '/q?x=1#frag')}, params = {"b": true})`,
    `requests.get(${text(base + '/q')}, params = {"id": listOf(1, 2, 3)})`,
    `requests.get(${text(base + '/q')}, params = {"id": ["a", "b"]})`,
    `requests.get(${text(base + '/q')}, params = mapOf<Text, Int>())`,
    'let bad: MutableMap<Text, Any> = mutableMapOf()', 'bad["t"] = Thread.currentThread()',
    catching(`requests.get(${text(base + '/q')}, params = bad)`, 'InvalidRequestError', 'unsupported value'),
  ], ['unsupported value'], seen => {
    const queries = seen.map(s => s.query);
    assert.equal(queries[0], '?a%20b=c%20d&amp=1%262%3D3&uni=%C3%A9%F0%9F%A6%A3&plus=a%2Bb');
    assert.deepEqual(queries.slice(1), ['?x=1&y=2', '?x=1&b=true', '?id=1&id=2&id=3', '?id=a&id=b', '']);
  });

  await scenario('text, bytes and form bodies', [
    `requests.post(${text(base + '/get')}, text = ${text('h\u00e9llo')})`,
    'let data = java.util.Base64.getDecoder().decode("AAH/gH8=")',
    `requests.post(${text(base + '/get')}, bytes = data)`,
    `requests.post(${text(base + '/get')}, bytes = data, headers = {"Content-Type": "image/png"})`,
    `requests.post(${text(base + '/get')}, form = {"a b": "c&d", "u": "\u00e9"})`,
    `requests.post(${text(base + '/get')}, form = {"n": listOf(1, 2)})`,
    'print("ok")',
  ], ['ok'], seen => {
    assert.match(seen[0].type, /^text\/plain/);
    assert.equal(seen[0].body, 'h\u00e9llo');
    assert.equal(seen[1].type, 'application/octet-stream');
    assert.deepEqual([...seen[1].raw], [0, 1, 255, 128, 127]);
    assert.equal(seen[2].type, 'image/png');
    assert.equal(seen[3].type, 'application/x-www-form-urlencoded');
    assert.equal(seen[3].body, 'a+b=c%26d&u=%C3%A9');
    assert.equal(seen[4].body, 'n=1&n=2');
  });

  await scenario('conflicting bodies', [
    `let u = ${text(base + '/get')}`,
    catching('requests.post(u, json = {"a": 1}, text = "b")', 'InvalidRequestError', 'json text'),
    catching('requests.post(u, text = "b", form = {"a": 1})', 'InvalidRequestError', 'text form'),
    catching('requests.post(u, bytes = "b".getBytes(), json = "c")', 'InvalidRequestError', 'bytes json'),
    'let message = try { requests.post(u, json = "a", text = "b").text } catch (e: RequestError) { e.getMessage() }', 'print(message)',
  ], ['json text', 'text form', 'bytes json', 'Give only one of json, text, bytes or form.'], seen => assert.equal(seen.length, 0));

  await scenario('invalid urls', [
    ...['ftp://secret-token/x', 'no scheme secret-token', 'http://', 'http://user:secret-token@127.0.0.1/', 'http://exa mple/secret-token'].flatMap((url, i) => [
      `let m${i} = try { requests.get(${text(url)}).text } catch (e: InvalidRequestError) { e.getMessage() }`,
      `print(m${i}.contains("secret-token"))`,
    ]),
  ], ['false', 'false', 'false', 'false', 'false'], seen => assert.equal(seen.length, 0));

  await scenario('response headers, bytes and text', [
    `let r = requests.get(${text(base + '/headers')})`,
    'print(r.headers["x-mixed-CASE"])', 'print(r.header("X-MIXED-case"))', 'print(r.headers["absent"] == null)', 'print(r.headers.has("SET-COOKIE"))', 'print(r.headers.has("absent"))',
    'print(r.headers.all("set-cookie"))', 'print(r.headers.all("absent").size())', 'print(r.headers.names().contains("x-mixed-case"))',
    'print(r.headers.toString().startsWith("Headers("))', 'print(r)',
    `let b = requests.get(${text(base + '/binary')})`, 'print(b.bytes.size)', 'print(b.bytes[1])', 'print(b.bytes[3])',
    `print(requests.get(${text(base + '/latin1')}).text)`,
    `print(requests.get(${text(base + '/badcharset')}).text)`,
    `print(requests.get(${text(base + '/utf8')}).text)`,
    `print(requests.get(${text(base + '/get')}, params = {"a": "b"}).url.endsWith("/get?a=b"))`,
    `print(requests.head(${text(base + '/get')}).bytes.size)`,
  ], ['one', 'one', 'true', 'true', 'false', '[a=1, b=2]', '0', 'true', 'true', 'Response(200)', '4', '-56', '65', 'caf\u00e9', 'caf\u00e9', 'caf\u00e9', 'true', '0'].map((line, i) => line));

  const closed = await new Promise(resolve => { const s = http.createServer(); s.listen(0, '127.0.0.1', () => { const port = s.address().port; s.close(() => resolve(port)); }); });
  const secret = `/private?token=hunter2`;
  await scenario('error kinds', [
    `let refused = ${text('http://127.0.0.1:' + closed + secret)}`,
    'try { requests.get(refused) } catch (e: TransportError) {',
    '    print(e.errorName())', '    print(e.getMessage().contains("hunter2"))', '    print(e.toString().contains("hunter2"))', '    print(e.original != null)', '    print(e.getCause() == null)',
    '}',
    catching('requests.get(refused)', 'RequestError', 'transport is a RequestError'),
    catching(`requests.get(${text(base + '/slow' + '?token=hunter2')}, timeout = 1)`, 'TimeoutError', 'slow headers'),
    'try { requests.get(' + text(base + '/stall') + ', timeout = 1) } catch (e: TimeoutError) { print(e.getMessage()) }',
    catching(`requests.get(${text(base + '/stall')}, timeout = 1)`, 'RequestError', 'timeout is a RequestError'),
    catching(`requests.get(${text(base + '/badjson')}).json()`, 'JsonError', 'malformed json'),
    `let failed = requests.get(${text(base + '/server-error' + '?token=hunter2')})`,
    'print(failed.status)', 'print(failed.ok)', 'print(failed.json()["retry"].asBool())',
    'try { failed.raiseForStatus() } catch (e: HttpStatusError) {',
    '    print(e.status)', '    print(e.response.text)', '    print(e.getMessage())', '    print(e)',
    '}',
    catching('failed.raiseForStatus()', 'RequestError', 'status is a RequestError'),
    `requests.get(${text(base + '/get')}).raiseForStatus()`, 'print("no error for 200")',
    'Thread.currentThread().interrupt()',
    catching(`requests.get(${text(base + '/get')})`, 'RequestError', 'interrupted'),
    'print(Thread.interrupted())',
  ], ['TransportError', 'false', 'false', 'true', 'true', 'transport is a RequestError', 'slow headers', 'HTTP request timed out after 1 seconds.', 'timeout is a RequestError', 'malformed json', '503', 'false', 'true', '503', '{"retry":true}', 'HTTP status 503', 'HttpStatusError: HTTP status 503', 'status is a RequestError', 'no error for 200', 'interrupted', 'true']);

  await scenario('authentication', [
    `let u = ${text(base + '/get')}`,
    'requests.get(u, auth = Auth.bearer("tok-1"))',
    `requests.get(u, auth = Auth.basic("ann", ${text('p\u00e4ss:1')}))`,
    'requests.post(u, json = {"a": 1}, auth = Auth.bearer("tok-2"), headers = {"Authorization": "ignored"})',
    catching('Auth.basic("a:b", "c")', 'InvalidRequestError', 'colon'),
    catching('requests.get(u, auth = Auth.bearer("x\\ny"))', 'InvalidRequestError', 'line break'),
    'print(Auth.bearer("tok-3"))',
  ], ['colon', 'line break', 'Auth(hidden)'], seen => {
    assert.equal(seen.length, 3);
    assert.equal(seen[0].headers.authorization, 'Bearer tok-1');
    assert.equal(seen[1].headers.authorization, 'Basic ' + Buffer.from('ann:p\u00e4ss:1', 'utf8').toString('base64'));
    assert.equal(seen[2].headers.authorization, 'Bearer tok-2');
  });

  await scenario('session headers and auth', [
    `let u = ${text(base + '/get')}`,
    'let s = Session(headers = {"X-Shared": "1", "X-Over": "session", "Accept": "text/x"}, auth = Auth.bearer("shared"))',
    's.get(u)',
    's.get(u, headers = {"x-over": "request"})',
    's.get(u, headers = {"Authorization": "Custom one"})',
    's.get(u, auth = Auth.basic("a", "b"))',
    's.post(u, json = {"a": 1})',
    'let typed = Session(headers = {"Content-Type": "application/x-custom"})',
    'typed.post(u, json = {"a": 1})',
    'typed.post(u, json = {"a": 1}, headers = {"Content-Type": "application/y"})',
    'requests.get(u)',
    catching('Session(headers = {"Bad Name": "x"})', 'InvalidRequestError', 'bad header'),
    catching('Session(headers = {"Host": "x"})', 'InvalidRequestError', 'managed header'),
  ], ['bad header', 'managed header'], seen => {
    assert.equal(seen[0].headers['x-shared'], '1');
    assert.equal(seen[0].headers['x-over'], 'session');
    assert.equal(seen[0].headers.accept, 'text/x');
    assert.equal(seen[0].headers.authorization, 'Bearer shared');
    assert.equal(seen[1].headers['x-over'], 'request');
    assert.equal(seen[2].headers.authorization, 'Custom one');
    assert.equal(seen[3].headers.authorization, 'Basic ' + Buffer.from('a:b').toString('base64'));
    assert.equal(seen[4].headers['x-shared'], '1');
    assert.match(seen[4].type, /application\/json/);
    assert.equal(seen[5].type, 'application/x-custom');
    assert.equal(seen[6].type, 'application/y');
    assert.equal(seen[7].headers['x-shared'], undefined);
    assert.equal(seen[7].headers.authorization, undefined);
  });

  await scenario('session cookies', [
    `let base = ${text(base)}`,
    'let s = Session()',
    'print(s.cookie("sid") == null)',
    's.get(base + "/set-cookie")',
    'print(s.cookie("sid"))', 'print(s.cookies()["theme"])', 'print(s.cookies().size())',
    's.get(base + "/other")',
    's.get(base + "/get")',
    'let t = Session()', 't.get(base + "/other")',
    'requests.get(base + "/set-cookie")', 'requests.get(base + "/other")',
    's.get(base + "/foreign-cookie")', 'print(s.cookie("evil") == null)',
    's.setCookie(base, "manual", "m1")', 's.get(base + "/other")', 'print(s.cookie("manual"))',
    's.clearCookies()', 'print(s.cookies().size())', 's.get(base + "/other")',
  ], ['true', 'abc', 'dark', '2', 'true', 'm1', '0'], seen => {
    const cookies = seen.map(r => r.headers.cookie);
    assert.equal(cookies[0], undefined);
    assert.equal(cookies[1], 'sid=abc');
    assert.ok(cookies[2].includes('sid=abc') && cookies[2].includes('theme=dark'), cookies[2]);
    assert.equal(cookies[3], undefined);
    assert.equal(cookies[4], undefined);
    assert.equal(cookies[5], undefined);
    assert.ok(!(cookies[6] || '').includes('evil'));
    assert.ok(cookies[7].includes('manual=m1') && cookies[7].includes('sid=abc'), cookies[7]);
    assert.equal(cookies[8], undefined);
  });

  await scenario('session timeouts and closing', [
    `let u = ${text(base)}`,
    'let quick = Session(timeout = 1)',
    catching('quick.get(u + "/slow")', 'TimeoutError', 'session timeout'),
    'print(quick.get(u + "/slow", timeout = 5).text)',
    catching('Session(timeout = 0)', 'InvalidRequestError', 'bad timeout'),
    catching('Session(connectTimeout = 0)', 'InvalidRequestError', 'bad connect timeout'),
    catching('quick.get(u + "/get", timeout = -1)', 'InvalidRequestError', 'bad request timeout'),
    'let s = Session(connectTimeout = 3)', 's.setCookie(u, "a", "b")', 's.close()', 'print(s.cookies().size())',
    catching('s.get(u + "/get")', 'InvalidRequestError', 'closed'),
    's.close()',
    'let used = Session().use { it.get(u + "/get").status }', 'print(used)',
  ], ['session timeout', 'late', 'bad timeout', 'bad connect timeout', 'bad request timeout', '0', 'closed', '200']);

  await scenario('one session across threads', [
    `let u: Text = ${text(base + '/other')}`,
    'let s: Session = Session(headers = {"X-T": "1"})', `s.setCookie(u, "c", "1")`,
    'let good: java.util.concurrent.atomic.AtomicInteger = java.util.concurrent.atomic.AtomicInteger(0)',
    'let threads = MutableList<Thread>()',
    'for i in 0 until 8 {',
    '    let t = Thread(Runnable { for j in 0 until 25 { if s.get(u, params = {"i": i, "j": j}).ok { good.incrementAndGet() } } })',
    '    threads.add(t)', '    t.start()',
    '}',
    'for t in threads { t.join() }', 'print(good.get())',
  ], ['200'], seen => {
    assert.equal(seen.length, 200);
    assert.equal(new Set(seen.map(r => r.query)).size, 200);
    assert.ok(seen.every(r => r.headers['x-t'] === '1' && r.headers.cookie === 'c=1'));
  });

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
