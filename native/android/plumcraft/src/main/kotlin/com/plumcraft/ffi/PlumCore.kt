package com.plumcraft.ffi

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.longOrNull
import java.nio.ByteBuffer
import java.util.concurrent.CompletableFuture
import java.util.concurrent.Executor
import java.util.concurrent.atomic.AtomicBoolean

class PlumException(message: String) : Exception(message)

/** Bytes lent by the core, read in place (a read-only direct buffer). [close] gives them back. */
class Blob(val buffer: ByteBuffer, val token: Long) : AutoCloseable {
    private val open = AtomicBoolean(true)

    override fun close() {
        if (open.getAndSet(false)) PlumNative.nativeRelease(token)
    }
}

/**
 * One core. The wake never calls into the VM: a waiter thread blocks in
 * [PlumNative.nativeAwait] and hops to [main] (on Android, the main thread's executor), where
 * [drain] polls and completes requests. [request] and [drain] belong to [main].
 */
class PlumCore(config: String = "{}", private val main: Executor) : AutoCloseable {
    val id: Long = PlumNative.nativeOpen(config.toByteArray(Charsets.UTF_8))
    private val lock = Any()
    private val pending = HashMap<Long, CompletableFuture<JsonElement>>()
    private val early = HashMap<Long, JsonObject>()
    private val scheduled = AtomicBoolean(false)
    private val running = AtomicBoolean(true)

    val events = ArrayList<JsonObject>()
    @Volatile var wakes = 0
        private set
    var polls = 0
        private set
    val eventsPerPoll = ArrayList<Int>()
    /** Each poll's raw bytes before parsing (measurements). */
    var onPoll: ((ByteArray) -> Unit)? = null

    private val waiter: Thread

    init {
        if (id == 0L) throw PlumException("plum_open: " + String(PlumNative.nativeLastError(), Charsets.UTF_8))
        waiter = Thread({
            while (running.get()) {
                if (PlumNative.nativeAwait(id, 250) && !scheduled.getAndSet(true)) {
                    wakes++
                    main.execute {
                        scheduled.set(false)
                        drain()
                    }
                }
            }
        }, "plumcraft-wake-$id")
        waiter.isDaemon = true
        waiter.start()
    }

    override fun close() {
        running.set(false)
        PlumNative.nativeClose(id)
        waiter.join(1000)
        synchronized(lock) {
            pending.values.forEach { it.completeExceptionally(PlumException("closed")) }
            pending.clear()
        }
    }

    fun send(json: ByteArray): Long = PlumNative.nativeRequest(id, json)

    /** A request, `{id?, method, params?}` as UTF-8 JSON; its reply's result. */
    fun request(json: ByteArray): CompletableFuture<JsonElement> {
        val f = CompletableFuture<JsonElement>()
        val rid = send(json)
        if (rid == 0L) {
            f.completeExceptionally(PlumException("the core is closed"))
            return f
        }
        val already = synchronized(lock) {
            early.remove(rid) ?: run {
                pending[rid] = f
                null
            }
        }
        if (already != null) complete(f, already)
        return f
    }

    fun call(method: String, params: JsonElement = JsonObject(emptyMap())): CompletableFuture<JsonElement> =
        request(JsonObject(mapOf("method" to JsonPrimitive(method), "params" to params)).toString().toByteArray(Charsets.UTF_8))

    fun command(command: String, params: JsonElement = JsonObject(emptyMap())): CompletableFuture<JsonElement> =
        call("command.run", JsonObject(mapOf("command" to JsonPrimitive(command), "params" to params)))

    private fun complete(f: CompletableFuture<JsonElement>, r: JsonObject) {
        if ((r["ok"] as? JsonPrimitive)?.booleanOrNull == true) {
            f.complete(r["result"] ?: JsonNull)
        } else {
            f.completeExceptionally(PlumException((r["error"] as? JsonPrimitive)?.contentOrNull ?: "error"))
        }
    }

    /** Poll until nothing is left. */
    fun drain() {
        while (true) {
            val bytes = PlumNative.nativePoll(id) ?: return
            polls++
            onPoll?.invoke(bytes)
            val obj = try {
                Json.parseToJsonElement(String(bytes, Charsets.UTF_8)) as? JsonObject
            } catch (e: Exception) {
                null
            } ?: continue
            for (r in (obj["replies"] as? JsonArray).orEmpty()) {
                val reply = r as? JsonObject ?: continue
                val rid = (reply["id"] as? JsonPrimitive)?.longOrNull ?: continue
                val f = synchronized(lock) {
                    pending.remove(rid) ?: run {
                        early[rid] = reply
                        null
                    }
                }
                if (f != null) complete(f, reply)
            }
            val evs = (obj["events"] as? JsonArray).orEmpty().mapNotNull { it as? JsonObject }
            if (evs.isNotEmpty()) {
                eventsPerPoll.add(evs.size)
                events.addAll(evs)
            }
        }
    }

    /** Borrow a store blob's bytes by handle, without a copy. */
    fun blob(handle: String): Blob {
        val token = LongArray(1)
        val buf = PlumNative.nativeBlob(id, handle.toByteArray(Charsets.UTF_8), token)
            ?: throw PlumException("no bytes for $handle")
        return Blob(buf.asReadOnlyBuffer(), token[0])
    }
}
