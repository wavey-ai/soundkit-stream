// Include your ringbuffer code here (assertPowerOfTwo, layout, ringbuffer, etc.)
// Include your decodeSoundKitFrameHeader here...

let rb;
let workingBuffer = new Uint8Array(0);
let currentPts = 0;
let isStreaming = false;
let streamId = null;

self.onmessage = (event) => {
    const { action, payload } = event.data;

    if (action === 'INIT_STREAM') {
        streamId = payload.streamId;
        currentPts = payload.fallbackPts || 0;
        
        // Initialize the consumer side of the ring buffer
        rb = ringbuffer(payload.sab, payload.frameSize, payload.maxFrames, Uint8Array);
        isStreaming = true;
        
        // Kick off the consumer loop
        consumeAndParse();
    }

    if (action === 'END_STREAM') {
        isStreaming = false;
        // Run one last time to clear out the ring buffer
        consumeAndParse();
    }
};

async function consumeAndParse() {
    if (!rb) return;

    let poppedChunk;
    let bytesAdded = false;

    // 1. Drain the ring buffer into our local working buffer
    while ((poppedChunk = rb.pop()) !== undefined) {
        workingBuffer = appendBuffer(workingBuffer, poppedChunk);
        bytesAdded = true;
    }

    // 2. Parse out all fully available SoundKit frames
    if (bytesAdded) {
        await extractFrames();
    }

    // 3. Keep polling if the stream is still alive
    if (isStreaming) {
        // Yield to the event loop, then poll again. 
        // Using setTimeout(..., 0) or requestAnimationFrame equivalents depending on worker type.
        setTimeout(consumeAndParse, 5); 
    }
}

async function extractFrames() {
    let offset = 0;
    const framesToStore = [];

    while (offset < workingBuffer.length) {
        // Not enough bytes for even the smallest base header? Wait for more data.
        if (offset + 8 > workingBuffer.length) break; 

        try {
            // We use a try-catch because decodeSoundKitFrameHeader might throw an 
            // 'incomplete header' error if it hits extended sizes we haven't downloaded yet.
            const header = decodeSoundKitFrameHeader(workingBuffer, offset);
            const totalFrameLength = header.headerBytes + header.payloadSize;

            // Do we have the full payload in the working buffer?
            if (offset + totalFrameLength > workingBuffer.length) {
                break; // Wait for the next chunk from the ring buffer
            }

            const pts = header.pts !== null ? header.pts : currentPts;
            
            framesToStore.push({
                byteOffset: offset,
                startFrame: pts,
                frameCount: header.frameCount,
                payloadSize: header.payloadSize,
                payload: workingBuffer.slice(offset + header.headerBytes, offset + totalFrameLength)
            });

            currentPts = pts + header.frameCount;
            offset += totalFrameLength;

        } catch (e) {
            // If it's a parsing error regarding incomplete bytes, break and wait.
            // If it's a corrupted stream, you would handle the abort sequence here.
            if (e.message.includes("incomplete")) {
                break; 
            } else {
                console.error("Fatal Stream Error:", e);
                isStreaming = false;
                return;
            }
        }
    }

    // 3. Store valid frames to IndexedDB
    if (framesToStore.length > 0) {
        await storeFramesToIndexedDB(streamId, framesToStore);
    }

    // 4. Shift the working buffer to discard processed bytes
    if (offset > 0) {
        workingBuffer = workingBuffer.slice(offset);
    }
}

// Utility: efficiently append typed arrays
function appendBuffer(buffer1, buffer2) {
    const tmp = new Uint8Array(buffer1.length + buffer2.length);
    tmp.set(buffer1, 0);
    tmp.set(buffer2, buffer1.length);
    return tmp;
}

function storeFramesToIndexedDB(streamId, frames) {
    return new Promise((resolve, reject) => {
        const request = indexedDB.open("SoundKitCache", 1);

        request.onupgradeneeded = (event) => {
            const db = event.target.result;
            // Create a store partitioned by streamId and startFrame
            if (!db.objectStoreNames.contains("frames")) {
                const store = db.createObjectStore("frames", { keyPath: ["streamId", "startFrame"] });
                store.createIndex("streamId", "streamId", { unique: false });
            }
        };

        request.onsuccess = (event) => {
            const db = event.target.result;
            const transaction = db.transaction("frames", "readwrite");
            const store = transaction.objectStore("frames");

            // Batch insert all parsed frames
            frames.forEach(frame => {
                store.put({ streamId, ...frame });
            });

            transaction.oncomplete = () => resolve();
            transaction.onerror = () => reject(transaction.error);
        };

        request.onerror = () => reject(request.error);
    });
}

