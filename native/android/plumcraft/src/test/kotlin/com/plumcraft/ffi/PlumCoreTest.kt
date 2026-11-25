package com.plumcraft.ffi

import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import org.junit.AfterClass
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test
import java.io.File
import java.util.concurrent.ExecutionException
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

/** The same sequence as the Swift package's tests, on the host JVM, then the measurements. */
class PlumCoreTest {
    companion object {
        /** The shell's "main thread". */
        val main = Executors.newSingleThreadExecutor { r -> Thread(r, "main") }

        @AfterClass @JvmStatic fun stop() = main.shutdown()

        /** Under the scratch root the Rust tests sweep (`plumcraft-test-<pid>`). */
        fun storeRoot(name: String): String {
            val d = File(System.getProperty("java.io.tmpdir"), "plumcraft-test-${ProcessHandle.current().pid()}/jvm-$name")
            d.mkdirs()
            return d.path
        }

        fun config(name: String, demo: Boolean = false) = buildJsonObject {
            put("store_root", storeRoot(name))
            put("demo", demo)
        }.toString()

        fun <T> onMain(f: () -> T): T = main.submit<T> { f() }.get(60, TimeUnit.SECONDS)

        fun <T> await(f: java.util.concurrent.CompletableFuture<T>): T = f.get(60, TimeUnit.SECONDS)

        fun obj(vararg kv: Pair<String, Any>): JsonObject = JsonObject(kv.associate { (k, v) -> k to json(v) })

        fun json(v: Any): JsonElement = when (v) {
            is JsonElement -> v
            is String -> JsonPrimitive(v)
            is Number -> JsonPrimitive(v)
            is Boolean -> JsonPrimitive(v)
            is List<*> -> JsonArray(v.map { json(it!!) })
            else -> error("$v")
        }

        fun stats(us: List<Double>): JsonObject {
            val s = us.sorted()
            fun at(q: Double) = s[Math.round((s.size - 1) * q).toInt()]
            return obj("n" to s.size, "median_us" to at(0.5), "p99_us" to at(0.99), "min_us" to s.first(), "max_us" to s.last())
        }

        fun report(name: String, v: JsonObject) = println("MEASURE " + obj("name" to name, "from" to "jvm", "v" to v))

        fun rssKb(): Long {
            val p = ProcessBuilder("ps", "-o", "rss=", "-p", ProcessHandle.current().pid().toString()).start()
            return p.inputStream.bufferedReader().readText().trim().toLongOrNull() ?: 0
        }
    }

    private fun PlumCore.cmd(id: String, params: JsonObject = JsonObject(emptyMap())): JsonElement =
        await(onMain { command(id, params) })

    private fun PlumCore.callMain(method: String, params: JsonObject = JsonObject(emptyMap())): JsonElement =
        await(onMain { call(method, params) })

    private fun idOf(e: JsonElement) = e.jsonObject["id"]!!.jsonPrimitive.content

    @Test
    fun openListBuildRunAndBorrow() {
        PlumCore(config("flow"), main).use { core ->
            val list = core.callMain("command.list").jsonArray
            assertTrue(list.any { it.jsonObject["id"]?.jsonPrimitive?.content == "node.add" })

            val lit = idOf(core.cmd("node.add", obj("kind" to "text.literal", "pos" to listOf(0, 0))))
            core.cmd("node.set", obj("node" to lit, "port" to "text", "value" to "The quick brown fox jumps over the lazy dog, then naps. 0123456\n"))
            val rep = idOf(core.cmd("node.add", obj("kind" to "text.repeat", "pos" to listOf(260, 0))))
            core.cmd("node.set", obj("node" to rep, "port" to "times", "value" to 1_000_000))
            val up = idOf(core.cmd("node.add", obj("kind" to "text.upper", "pos" to listOf(260, 160))))
            core.cmd("wire.connect", obj("from" to obj("node" to lit, "port" to "text"), "to" to obj("node" to rep, "port" to "text")))
            core.cmd("wire.connect", obj("from" to obj("node" to lit, "port" to "text"), "to" to obj("node" to up, "port" to "text")))
            val g = core.callMain("graph.get").jsonObject
            assertEquals(3, g["nodes"]!!.jsonObject.size)
            assertEquals(2, g["wires"]!!.jsonArray.size)

            core.cmd("run.start", obj("watch" to listOf(obj("node" to rep, "port" to "text"))))
            val done = core.callMain("spike.waitRun", obj("timeout_ms" to 30_000)).jsonObject
            val nodes = done["nodes"]!!.jsonObject
            assertEquals("done", nodes[rep]!!.jsonObject["state"]!!.jsonPrimitive.content)
            assertEquals("done", nodes[up]!!.jsonObject["state"]!!.jsonPrimitive.content)
            val kinds = onMain { core.events.mapNotNull { it["kind"]?.jsonPrimitive?.content }.toSet() }
            assertTrue("$kinds", "node.status" in kinds && "run.reply" in kinds)
            assertTrue(core.wakes > 0)

            val handle = done["watch"]!!.jsonArray[0].jsonObject["full"]!!.jsonObject["handle"]!!.jsonPrimitive.content
            val before = PlumNative.nativeLeases()
            core.blob(handle).use { b ->
                assertTrue(b.buffer.isDirect && b.buffer.isReadOnly)
                assertEquals(64_000_000, b.buffer.capacity())
                val head = ByteArray(19).also { b.buffer.duplicate().get(it) }
                assertEquals("The quick brown fox", String(head))
                assertEquals(before + 1, PlumNative.nativeLeases())
            }
            assertEquals(before, PlumNative.nativeLeases())
        }
    }

    @Test
    fun hostileAndClose() {
        val core = PlumCore(config("hostile"), main)
        for (bad in listOf(ByteArray(0), byteArrayOf(-1, -2), "{nope".toByteArray(), """{"method":"no.such"}""".toByteArray())) {
            try {
                await(onMain { core.request(bad) })
                fail("answered ok")
            } catch (e: ExecutionException) {
                assertTrue(e.cause is PlumException)
            }
        }
        assertEquals(0L, PlumNative.nativeRequest(424242, "{}".toByteArray()))
        assertEquals(null, PlumNative.nativeBlob(core.id, "b3:raw:00".toByteArray(), LongArray(1)))
        assertEquals(-3, PlumNative.nativeRelease(0))
        core.close()
        assertEquals(-1, PlumNative.nativeClose(core.id))
        assertEquals(null, PlumNative.nativePoll(core.id))
        assertEquals(false, PlumNative.nativeAwait(core.id, 10))
        try {
            PlumCore("""{"engine":"vectorcraft"}""", main)
            fail("opened")
        } catch (e: PlumException) {
            assertTrue(e.message!!.contains("text engine only"))
        }
    }

    @Test
    fun requestLatency() {
        PlumCore(config("latency", demo = true), main).use { core ->
            val ping = """{"method":"ping"}""".toByteArray()
            val move = """{"method":"command.run","params":{"command":"node.move","params":{"id":"shout","pos":[10,10]}}}""".toByteArray()
            for ((name, req) in listOf("latency.ping" to ping, "latency.command.node.move" to move)) {
                repeat(200) { await(onMain { core.request(req) }) }
                val us = ArrayList<Double>()
                repeat(2000) {
                    val t0 = System.nanoTime()
                    await(onMain { core.request(req) })
                    us.add((System.nanoTime() - t0) / 1e3)
                }
                report(name, stats(us))
            }
        }
    }

    @Serializable data class End(val node: String, val port: String)
    @Serializable data class Wire(val from: End, val to: End)
    @Serializable data class Node(val kind: String, val pos: List<Double>)
    @Serializable data class Graph(val nodes: Map<String, Node>, val wires: List<Wire>)
    @Serializable data class Reply(val id: Long, val ok: Boolean, val result: Graph? = null)
    @Serializable data class Poll(val replies: List<Reply>)

    @Test
    fun graphGetJsonCost() {
        val typed = Json { ignoreUnknownKeys = true }
        PlumCore(config("scene"), main).use { core ->
            for ((n, w) in listOf(500 to 800, 2000 to 3200)) {
                core.callMain("spike.scene", obj("nodes" to n, "wires" to w))
                var raw = ByteArray(0)
                onMain { core.onPoll = { raw = it } }
                val get = """{"method":"graph.get"}""".toByteArray()
                val e2e = ArrayList<Double>()
                val tree = ArrayList<Double>()
                val classes = ArrayList<Double>()
                repeat(40) { i ->
                    val t0 = System.nanoTime()
                    val g = await(onMain { core.request(get) })
                    val t1 = System.nanoTime()
                    Json.parseToJsonElement(String(raw, Charsets.UTF_8))
                    val t2 = System.nanoTime()
                    val p = typed.decodeFromString(Poll.serializer(), String(raw, Charsets.UTF_8))
                    val t3 = System.nanoTime()
                    assertEquals(n, g.jsonObject["nodes"]!!.jsonObject.size)
                    assertEquals(n, p.replies[0].result!!.nodes.size)
                    if (i >= 10) {
                        e2e.add((t1 - t0) / 1e3)
                        tree.add((t2 - t1) / 1e3)
                        classes.add((t3 - t2) / 1e3)
                    }
                }
                onMain { core.onPoll = null }
                report("scene.$n", obj("nodes" to n, "wires" to w, "bytes" to raw.size, "end_to_end_jsonelement" to stats(e2e),
                    "decode_jsonelement" to stats(tree), "decode_serializable" to stats(classes)))
            }
        }
    }

    @Test
    fun blob64MB() {
        PlumCore(config("blob"), main).use { core ->
            val p = core.callMain("spike.publish", obj("bytes" to (64 shl 20), "seed" to 11)).jsonObject
            val handle = p["handle"]!!.jsonPrimitive.content
            val rss0 = rssKb()
            val t0 = System.nanoTime()
            val b = core.blob(handle)
            val obtain = (System.nanoTime() - t0) / 1e3
            val rss1 = rssKb()
            val t1 = System.nanoTime()
            var sum = 0L
            var i = 0
            while (i < b.buffer.capacity()) {
                sum += (b.buffer.get(i).toInt() and 0xff)
                i += 16 shl 10
            }
            val touch = (System.nanoTime() - t1) / 1e3
            val rss2 = rssKb()
            val t2 = System.nanoTime()
            val again = core.blob(handle)
            val second = (System.nanoTime() - t2) / 1e3
            assertEquals(64 shl 20, b.buffer.capacity())
            report("blob.64MiB.mapped", obj("len" to b.buffer.capacity(), "obtain_us" to obtain, "second_borrow_us" to second, "touch_us" to touch,
                "direct" to b.buffer.isDirect, "rss_kb" to listOf(rss0, rss1, rss2), "sum" to sum))
            again.close()
            b.close()
        }
    }

    @Test
    fun wakesAndEventsDuringARun() {
        for (n in listOf(100, 500)) {
            PlumCore(config("run-$n"), main).use { core ->
                core.callMain("spike.scene", obj("nodes" to n, "wires" to n - 1))
                core.cmd("node.set", obj("node" to "n1", "port" to "text", "value" to "a corgi"))
                val (w0, p0, e0) = onMain { Triple(core.wakes, core.polls, core.eventsPerPoll.size) }
                val t0 = System.nanoTime()
                core.cmd("run.start")
                core.callMain("spike.waitRun", obj("timeout_ms" to 120_000))
                val secs = (System.nanoTime() - t0) / 1e9
                val (wakes, polls, per) = onMain { Triple(core.wakes - w0, core.polls - p0, core.eventsPerPoll.drop(e0)) }
                val events = per.sum()
                val stats = core.callMain("spike.stats").jsonObject
                report("run.$n", obj("nodes" to n, "seconds" to secs, "events" to events, "wakes" to wakes, "polls" to polls,
                    "wakes_per_s" to wakes / secs, "events_per_poll" to obj("mean" to events.toDouble() / maxOf(per.size, 1), "max" to (per.maxOrNull() ?: 0)),
                    "core" to stats))
                assertTrue(events > 2 * n - 10)
            }
        }
    }
}
