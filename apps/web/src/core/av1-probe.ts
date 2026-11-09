/** Whether this browser decodes AV1 power-efficiently (in hardware). `canPlayType` and `isTypeSupported` pass
 * 8-bit AV1 wherever a software decoder exists, which drops frames on a weak CPU; MediaCapabilities says whether
 * decoding is power-efficient. */

// A 1080p AV1 stream at a typical encode's rate, as Media Source plays it.
const AV1_1080P: MediaDecodingConfiguration = {
  type: "media-source",
  video: { contentType: 'video/mp4; codecs="av01.0.08M.08"', width: 1920, height: 1080, bitrate: 8_000_000, framerate: 24 },
};

/** Null until MediaCapabilities answers, and for good where it does not exist or fails: the probe then decides. */
let powerEfficient: boolean | null = null;

/** Asks once; `keepsAv1` reads the answer from then on. */
export async function probeAv1(caps: MediaCapabilities | undefined = globalThis.navigator?.mediaCapabilities): Promise<void> {
  if (!caps?.decodingInfo) return;
  try {
    const info = await caps.decodingInfo(AV1_1080P);
    powerEfficient = info.supported && info.powerEfficient;
  } catch {
    // No answer keeps the canPlayType probe's.
  }
}

/** AV1 stays offered unless MediaCapabilities said it decodes only in software. */
export const keepsAv1 = (): boolean => powerEfficient !== false;
