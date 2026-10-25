import { expect, test } from "bun:test";
import { downloadProblem, downloadWordsFailed, reported } from "./download-status";

test("a refused download op says so in one line, and the next one that goes through clears it", async () => {
  await reported(Promise.reject(new Error("a kids profile cannot download")), downloadWordsFailed.add);
  expect(downloadProblem()).toBe("Couldn't start this download. Try again.");
  await reported(Promise.resolve({ ids: ["d1"] }), downloadWordsFailed.add);
  expect(downloadProblem()).toBeNull();
});
