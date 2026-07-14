package dev.bharatcode.example

import dev.bharatcode.sdk.Client

fun main() {
    val client = Client()
    val pong = client.ping("aaif.io")
    println(pong.message)
}
