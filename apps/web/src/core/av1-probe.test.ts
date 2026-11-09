import { expect, test } from "bun:test";
import { keepsAv1, probeAv1 } from "./av1-probe";

const answering = (info: Partial<MediaCapabilitiesDecodingInfo> | Error, asked: MediaDecodingConfiguration[] = []) =>
  ({
    decodingInfo: async (config: MediaDecodingConfiguration) => {
      asked.push(config);
      if (info instanceof Error) throw info;
      return info as MediaCapabilitiesDecodingInfo;
    },
  }) as unknown as MediaCapabilities;

test("AV1 stays offered until MediaCapabilities says otherwise, and where it cannot", async () => {
  expect(keepsAv1()).toBe(true);
  await probeAv1(undefined);
  expect(keepsAv1()).toBe(true);
  await probeAv1(answering(new Error("no answer")));
  expect(keepsAv1()).toBe(true);
});

test("software-only AV1 is dropped, power-efficient AV1 is kept", async () => {
  const asked: MediaDecodingConfiguration[] = [];
  await probeAv1(answering({ supported: true, smooth: true, powerEfficient: false }, asked));
  expect(keepsAv1()).toBe(false);
  expect(asked[0]?.video?.contentType).toBe('video/mp4; codecs="av01.0.08M.08"');
  await probeAv1(answering({ supported: true, smooth: true, powerEfficient: true }));
  expect(keepsAv1()).toBe(true);
  await probeAv1(answering({ supported: false, smooth: false, powerEfficient: false }));
  expect(keepsAv1()).toBe(false);
});
