package com.mrsobakin.bark

import java.io.ByteArrayOutputStream
import java.util.zip.CRC32
import kotlin.random.Random

/**
 * Writes Opus audio into an OGG container entirely in memory.
 *
 * Produces a valid OGG Opus stream from packets obtained through Android's
 * MediaCodec Opus encoder. The sequence is:
 *   1. [writeOpusHead]  – Opus identification header (from encoder CSD-0)
 *   2. [writeOpusTags]  – comment header
 *   3. [writeAudioPacket] – one or more audio data pages (call per packet)
 *   4. [close]          – finalises with an end-of-stream page
 *
 * Every audio data page carries exactly one Opus packet (simpler book-keeping).
 */
class OggOpusWriter(
    /** OGG bitstream serial number. Randomised to avoid collision. */
    private val serialNumber: Int = Random.nextInt(),
    /** Input sample rate of the PCM that was fed to the Opus encoder. */
    private val inputSampleRate: Int = 16000,
) {
    private val baos = ByteArrayOutputStream()
    private var pageNo = 0
    private var granulePos = 0L
    private var audioPacketCount = 0

    /** Whether any audio data packets have been written. */
    val hasAudio: Boolean get() = audioPacketCount > 0

    /** Returns the complete OGG stream as a byte array. */
    fun toByteArray(): ByteArray = baos.toByteArray()

    /**
     * Write the Opus identification header as the first OGG page.
     * @param csd The codec-specific data obtained from the MediaCodec Opus
     *            encoder (OpusHead packet, typically 19 bytes).
     */
    fun writeOpusHead(csd: ByteArray) {
        writePage(listOf(csd), 0L, 0x02) // BOS – first page of the stream
    }

    /**
     * Write the Opus comment header as the second OGG page.
     */
    fun writeOpusTags(vendor: String = "Bark") {
        val vBytes = vendor.encodeToByteArray()
        val os = ByteArrayOutputStream()
        os.write("OpusTags".encodeToByteArray())
        writeLe32(os, vBytes.size)       // vendor string length
        os.write(vBytes)                 // vendor string
        writeLe32(os, 0)                 // user comment list length = 0
        writePage(listOf(os.toByteArray()), 0L, 0)
    }

    /**
     * Write one Opus audio packet as an OGG page.
     *
     * @param packet Raw Opus packet from the MediaCodec encoder.
     * @param inputSampleCount Number of input PCM samples (at [inputSampleRate])
     *                         that were consumed to produce this packet.
     */
    fun writeAudioPacket(packet: ByteArray, inputSampleCount: Int) {
        // OGG granule position is always in 48 kHz units for Opus.
        val inc = inputSampleCount.toLong() * 48000L / inputSampleRate
        granulePos += inc
        writePage(listOf(packet), granulePos, 0)
        audioPacketCount++
    }

    /**
     * Finalise the stream.  Writes an empty end-of-stream page so that
     * decoders know there is no more audio data.
     */
    fun close() {
        writePage(emptyList(), granulePos, 0x04) // EOS
    }

    // ------------------------------------------------------------------
    //  Private helpers
    // ------------------------------------------------------------------

    /** Assemble and write a single OGG page. */
    private fun writePage(packets: List<ByteArray>, granule: Long, flags: Int) {
        // Build the segment (lacing) table.
        val segTable = mutableListOf<Int>()
        for (p in packets) {
            var rem = p.size
            while (rem >= 255) {
                segTable.add(255)
                rem -= 255
            }
            segTable.add(rem)
        }
        val segCount = segTable.size

        // Concatenate all packet data.
        val totalData = packets.sumOf { it.size }
        val data = ByteArray(totalData).apply {
            var pos = 0
            for (p in packets) {
                p.copyInto(this, pos)
                pos += p.size
            }
        }

        val pageSize = 27 + segCount + data.size
        val page = ByteArray(pageSize)

        // Capture pattern "OggS"
        byteArrayOf(0x4F, 0x67, 0x67, 0x53).copyInto(page, 0)

        page[4] = 0                    // stream structure version
        page[5] = flags.toByte()       // header type flag

        // Granule position (signed 64-bit LE)
        writeLe64(page, 6, granule)
        // Bitstream serial number (LE32)
        writeLe32(page, 14, serialNumber)
        // Page sequence number (LE32)
        writeLe32(page, 18, pageNo)
        // CRC-32 placeholder – bytes 22-25 stay 0 during computation

        // Number of page segments
        page[26] = segCount.toByte()

        // Segment table
        for (i in 0 until segCount) {
            page[27 + i] = segTable[i].toByte()
        }

        // Packet data
        data.copyInto(page, 27 + segCount)

        // CRC-32 over the entire page (with checksum field = 0)
        val checksum = CRC32().apply { update(page) }.value.toInt()
        writeLe32(page, 22, checksum)

        baos.write(page)
        pageNo++
    }

    companion object {
        /** Write a 32-bit little-endian integer into a byte array. */
        private fun writeLe32(dest: ByteArray, off: Int, v: Int) {
            dest[off] = (v and 0xFF).toByte()
            dest[off + 1] = ((v shr 8) and 0xFF).toByte()
            dest[off + 2] = ((v shr 16) and 0xFF).toByte()
            dest[off + 3] = ((v shr 24) and 0xFF).toByte()
        }

        /** Write a 64-bit little-endian integer into a byte array. */
        private fun writeLe64(dest: ByteArray, off: Int, v: Long) {
            dest[off] = (v and 0xFF).toByte()
            dest[off + 1] = ((v shr 8) and 0xFF).toByte()
            dest[off + 2] = ((v shr 16) and 0xFF).toByte()
            dest[off + 3] = ((v shr 24) and 0xFF).toByte()
            dest[off + 4] = ((v shr 32) and 0xFF).toByte()
            dest[off + 5] = ((v shr 40) and 0xFF).toByte()
            dest[off + 6] = ((v shr 48) and 0xFF).toByte()
            dest[off + 7] = ((v shr 56) and 0xFF).toByte()
        }

        /** Write a 32-bit little-endian integer to a stream. */
        private fun writeLe32(os: ByteArrayOutputStream, v: Int) {
            os.write(v and 0xFF)
            os.write((v shr 8) and 0xFF)
            os.write((v shr 16) and 0xFF)
            os.write((v shr 24) and 0xFF)
        }
    }
}
