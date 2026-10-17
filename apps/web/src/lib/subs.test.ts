import { expect, test } from "bun:test";
import { toVtt } from "./subs";

test("srt becomes vtt", () => {
  const srt = "﻿1\r\n00:00:01,500 --> 00:00:03,000\r\nWinter is coming.\r\n";
  expect(toVtt(srt)).toBe("WEBVTT\n\n1\n00:00:01.500 --> 00:00:03.000\nWinter is coming.\n");
});

test("vtt passes through", () => {
  expect(toVtt("WEBVTT\n\n00:01.000 --> 00:02.000\nHi")).toBe("WEBVTT\n\n00:01.000 --> 00:02.000\nHi");
});
