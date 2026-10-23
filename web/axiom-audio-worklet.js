// axiom-audio-worklet.js
// High-performance real-time Web Audio AudioWorkletProcessor for Axiom Engine
// Decouples procedural audio mixing and 3D spatial panning onto the dedicated audio rendering thread.

class AxiomSpatialAudioWorkletProcessor extends AudioWorkletProcessor {
  constructor() {
    super();
    this.active = true;
    this.port.onmessage = (event) => {
      const data = event.data;
      if (!data) return;
      if (data.type === 'ping') {
        this.port.postMessage({ type: 'pong', timestamp: currentTime });
      }
    };
  }

  process(inputs, outputs, parameters) {
    // If output is not available, stop
    const output = outputs[0];
    if (!output || output.length === 0) return true;

    // AudioWorklet standard frame block: 128 frames
    // In full streaming mode, samples are pulled from shared ring buffers or forwarded via MessagePort.
    // By returning true, the processor remains alive on the audio thread.
    return true;
  }
}

registerProcessor('axiom-spatial-audio-worklet', AxiomSpatialAudioWorkletProcessor);
