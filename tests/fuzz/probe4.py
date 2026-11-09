"""Fourth round: numeric ranges, class conveniences, library corners."""
import os, subprocess, shutil, sys
HERE = __import__('tempfile').mkdtemp().replace(chr(92), '/')
COMPILER = os.environ.get('MAMMOTH', 'target/release/mammoth.exe')
cases = {
'long_range_loop': 'for i in 1L..3L { print(i) }\n',
'char_range_value': "print(('a'..'c').toList())\n",
'long_range_value': 'print((1L..3L).toList())\n',
'range_step_value': 'print((0..10 step 5).toList())\n',
'range_reversed': 'print((1..3).reversed())\n',
'range_first_last': 'let r = 1..3\nprint(r.first())\nprint(r.last())\n',
'int_range_type': 'let r: IntRange = 1..3\nprint(r.size)\n',
'until_step_loop': 'for i in 0 until 10 step 3 { print(i) }\n',
'downto_step': 'for i in 10 downTo 0 step 5 { print(i) }\n',
'repeat': 'repeat(2) { print(it) }\n',
'times_label': 'outer@ for i in 0..1 { for j in 0..1 { if j == 1 { continue@outer }\n print(i) } }\n',
'operator_times_compound': 'class V(let n: Int) { fun plus(o: V): V = V(n + o.n) }\nvar v = V(1)\nv += V(2)\nprint(v.n)\n',
'operator_index_compound': 'let l = mutableListOf(1)\nl[0] += 2\nl[0]++\nprint(l)\n',
'operator_map_compound': 'let m = mutableMapOf("a" to 1)\nm["a"] = m["a"]!! + 1\nprint(m)\n',
'operator_contains_custom': 'class B { fun contains(x: Int): Bool = x > 0 }\nprint(1 in B())\nprint(-1 !in B())\n'.replace('!in', 'not in'),
'operator_rangeto': 'data class V(let n: Int) implements Comparable<V> { override fun compareTo(o: V): Int = n - o.n }\nprint(V(2) in V(1)..V(3))\n',
'operator_invoke': 'class F { fun invoke(x: Int): Int = x + 1 }\nlet f = F()\nprint(f(1))\n',
'operator_get_two': 'class G { fun get(a: Int, b: Int): Int = a + b }\nprint(G()[1, 2])\n',
'data_equals_hash': 'data class P(let a: Int, let b: Text)\nprint(P(1, "x") == P(1, "x"))\nprint(P(1, "x").hashCode() == P(1, "x").hashCode())\nprint(P(1, "x") != P(2, "x"))\n',
'data_copy_nested': 'data class A(let n: Int)\ndata class B(let a: A, let t: Text)\nprint(B(A(1), "x").copy(t = "y"))\n',
'data_list_field': 'data class P(let xs: MutableList<Int>)\nprint(P(mutableListOf(1)))\n',
'data_nullable_field': 'data class P(let a: Text?)\nprint(P(null))\nprint(P(null) == P(null))\n',
'data_generic_eq': 'data class P<T>(let a: T)\nprint(P(1) == P(1))\nprint(P("a"))\n',
'data_inherit': 'open class B\ndata class P(let a: Int) extends B\nprint(P(1))\n',
'class_tostring_default': 'class K\nprint(K().toString().startsWith("Mammoth") or true)\n',
'class_equals_identity': 'class K\nlet a = K()\nprint(a == a)\nprint(a == K())\nprint(a === a)\n',
'identity_not': 'class K\nlet a = K()\nprint(a !== K())\n',
'static_like': 'class K { companion object { var n = 0\n fun inc() { n += 1 } } }\nK.inc()\nprint(K.n)\n',
'init_multiple': 'class K(let a: Int) { var b = 0\n init { b = a }\n var c = b + 1\n init { c += 1 } }\nprint(K(1).c)\n',
'default_uses_earlier': 'fun f(a: Int, b: Int = a + 1): Int = a + b\nprint(f(1))\n',
'named_mixed': 'fun f(a: Int, b: Int, c: Int): Int = a * 100 + b * 10 + c\nprint(f(1, c = 3, b = 2))\n',
'vararg_mix': 'fun f(p: Text, vararg r: Int): Int = r.size\nprint(f("a"))\nprint(f("a", 1, 2))\nprint(f("a", *[1, 2, 3]))\n',
'lambda_two_params': 'fun f(g: (Int, Int) -> Int): Int = g(1, 2)\nprint(f { a, b -> a + b })\n',
'lambda_underscore': 'fun f(g: (Int, Int) -> Int): Int = g(1, 2)\nprint(f { _, b -> b })\n',
'lambda_multiline_value': 'print(listOf(1).map {\n val a = it + 1\n a * 2\n})\n',
'lambda_if_value': 'print(listOf(1).map { if it > 0 { "p" } else { "n" } })\n',
'lambda_when_value': 'print(listOf(1).map { when (it) {\n 1 -> "a"\n else -> "b"\n} })\n',
'lambda_generic_ret': 'fun f<T>(g: () -> T): T = g()\nprint(f { 1 } + 1)\nprint(f { "a" }.length)\n',
'function_type_alias_call': 'typealias Op = (Int, Int) -> Int\nfun ap(o: Op): Int = o(2, 3)\nprint(ap { a, b -> a * b })\n',
'recursion_lambda_field': 'class K { var f: (Int) -> Int = { 0 }\n init { f = { n -> if n == 0 { 0 } else { n + f(n - 1) } } } }\nprint(K().f(3))\n',
'map_of_lambdas': 'let m = mapOf("a" to { x: Int -> x + 1 })\nprint(m["a"]!!(1))\n',
'nested_generics': 'let m = mutableMapOf<Text, MutableList<MutableList<Int>>>()\nm["a"] = mutableListOf(mutableListOf(1))\nprint(m["a"]!![0][0] + 1)\n',
'list_of_pairs_destructure': 'for (a, b) in listOf(1 to "x") { print(b) }\n',
'map_destructure_lambda': 'mapOf("a" to 1).forEach { (k, v) -> print(k) }\n',
'sorted_multi': 'data class P(let a: Int, let b: Text)\nprint(listOf(P(1, "b"), P(1, "a")).sortedWith(compareBy { it.b }).map { it.b })\n',
'then_by': 'data class P(let a: Int, let b: Text)\nprint(listOf(P(1, "b"), P(0, "a")).sortedWith(compareBy<P, Int> { it.a }.thenBy { it.b }).size)\n',
'max_min_of': 'print(listOf(1, 2).maxOf { it * 2 })\nprint(listOf(1, 2).minOf { it * 2 })\n',
'sum_empty': 'print(mutableListOf<Int>().sum())\n',
'average': 'print(listOf(1, 2).average())\n',
'flatten': 'print(listOf(listOf(1), listOf(2)).flatten())\n',
'list_contains_all': 'print(listOf(1, 2).containsAll(listOf(1)))\n',
'list_index_of_first': 'print(listOf(1, 2).indexOfFirst { it == 2 })\nprint(listOf(1, 2).indexOfLast { it == 1 })\n',
'list_last_or_null': 'print(mutableListOf<Int>().lastOrNull())\nprint(listOf(1).lastOrNull { it > 0 })\n',
'list_drop_last': 'print(listOf(1, 2).dropLast(1))\nprint(listOf(1, 2).takeLast(1))\n',
'list_is_not_empty': 'print(listOf(1).isNotEmpty())\n',
'list_get_or_else': 'print(listOf(1).getOrElse(5) { 0 })\nprint(listOf(1).getOrNull(5))\n',
'list_fill_sort': 'let l = mutableListOf(2, 1)\nl.sort()\nprint(l)\nl.reverse()\nprint(l)\n',
'list_remove_all': 'let l = mutableListOf(1, 2)\nl.removeAll { it == 1 }\nprint(l)\nl.removeIf { it == 2 }\nprint(l)\n',
'list_to_array': 'print(listOf(1).toTypedArray().size)\nprint(listOf(1).toIntArray().size)\n',
'list_to_map': 'print(listOf(1).associate { it to it + 1 })\n',
'list_count_empty': 'print(listOf(1).count())\n',
'list_reduce_right': 'print(listOf(1, 2).foldRight(0) { a, b -> a + b })\n',
'list_step_window': 'print(listOf(1, 2, 3).zipWithNext())\n',
'set_add_remove': 'let s = mutableSetOf(1)\ns.remove(1)\nprint(s.isEmpty())\nprint(setOf(1, 2).map { it + 1 })\n',
'set_plus': 'print(setOf(1) + setOf(2))\n',
'map_to_sorted': 'print(mapOf("b" to 1, "a" to 2).toSortedMap())\n',
'map_values_sum': 'print(mapOf("a" to 1).values.sum())\n',
'map_any_all': 'print(mapOf("a" to 1).any { it.value > 0 })\nprint(mapOf("a" to 1).all { it.value > 0 })\nprint(mapOf("a" to 1).count { it.value > 0 })\n',
'map_filter': 'print(mapOf("a" to 1).filter { it.value > 0 })\n',
'map_max_by': 'print(mapOf("a" to 1).maxByOrNull { it.value }?.key)\n',
'map_put_all': 'let m = mutableMapOf("a" to 1)\nm.putAll(mapOf("b" to 2))\nprint(m.size)\n',
'map_get_or_default_null': 'print(mapOf("a" to 1)["z"] ?: 9)\n',
'map_keys_list': 'print(mapOf("a" to 1).keys.toList())\nprint(mapOf("a" to 1).values.toList())\n',
'text_substring_before': 'print("a=b".substringBefore("="))\nprint("a=b".substringAfter("="))\n',
'text_remove_prefix': 'print("ab".removePrefix("a"))\nprint("ab".removeSuffix("b"))\n',
'text_capitalize': 'print("ab".replaceFirstChar { it.uppercase() })\n',
'text_is_digit_all': 'print("12".all { it.isDigit() })\nprint("12".any { it == \'1\' })\nprint("12".count { it == \'1\' })\n',
'text_split_regex': 'print("a1b".split(Regex("[0-9]")))\n',
'text_chunked': 'print("abcd".chunked(2))\n',
'text_to_long_double': 'print("1".toLong())\nprint("x".toDoubleOrNull())\nprint("true".toBoolean())\n',
'text_compare_ignore': 'print("a".compareTo("B", true))\n',
'text_format_multi': 'print("%s-%d".format("a", 1))\n',
'text_lines_trim': 'print("  a\\n b".trimIndent())\n',
'text_join': 'print(listOf("a").joinToString(separator = "-", prefix = "<"))\n',
'text_contains_ignore': 'print("Ab".contains("a", ignoreCase = true))\n',
'text_replace_regex': 'print("a1".replace(Regex("[0-9]"), "x"))\n',
'text_char_at': 'print("ab"[1])\nprint("ab".get(0))\n',
'text_concat_num': 'print("a" + 1.toString())\nprint("v: {1 + 1}")\n',
'char_conversions': 'print(\'a\'.toString())\nprint(\'a\'.uppercase())\nprint(\'a\' < \'b\')\n',
'num_round': 'print(1.5.toInt())\nprint(Math.round(1.5))\nprint(1.6.roundToInt())\n',
'num_pow': 'print(Math.pow(2.0, 3.0))\nprint(2.0.pow(3))\n',
'num_abs_ext': 'print((-1).absoluteValue)\nprint(-1.5.absoluteValue)\n',
'num_is_nan': 'print((0.0 / 0.0).isNaN())\n',
'num_hex_string': 'print(255.toString(16))\nprint(Integer.toHexString(255))\n',
'num_compare_mixed': 'print(1 < 2L)\nprint(1 == 1)\nprint(1.0 > 0)\n',
'num_bool_ops': 'print(true and false)\nprint(not true)\nprint(true or false)\n',
'num_max_min_fn': 'print(maxOf(1, 2))\nprint(minOf(1.0, 2.0))\nprint(maxOf(1, 2, 3))\n',
}
os.makedirs(HERE + '/ko4', exist_ok=True)
only = sys.argv[1:]
ok, rej, bad = 0, [], []
for name, src in cases.items():
    if only and name not in only:
        continue
    path = HERE + '/ko4/' + name + '.mam'
    open(path, 'w', encoding='utf-8').write(src)
    out = HERE + '/ko4/o'
    shutil.rmtree(out, ignore_errors=True)
    p = subprocess.run([COMPILER, path, out], capture_output=True, timeout=60, stdin=subprocess.DEVNULL)
    if p.returncode != 0:
        lines = p.stderr.decode('utf-8', 'replace').strip().splitlines()
        msg = lines[0].split(': ', 1)[-1] if lines else f'exit {p.returncode}'
        rej.append(f'{name}: {msg[:90]}')
        continue
    r = subprocess.run(['java', '-Xverify:all', '-cp', out, 'Main'], capture_output=True, timeout=30, stdin=subprocess.DEVNULL)
    if r.returncode != 0:
        bad.append(f'{name}: ' + r.stderr.decode('utf-8', 'replace').strip().splitlines()[0][:150])
    else:
        ok += 1
        if only or os.environ.get('SHOW'):
            print(name, '=>', r.stdout.decode('utf-8', 'replace').strip().replace('\r', '').replace('\n', ' | ')[:120])
print('ok', ok, 'rejected', len(rej), 'runtime', len(bad))
print('--- rejected'); print('\n'.join(rej))
print('--- runtime'); print('\n'.join(bad))
