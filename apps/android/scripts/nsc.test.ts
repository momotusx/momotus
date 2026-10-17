import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { aarPath, generate, verifierHosts, xmlPath } from "./nsc";

test("network_security_config.xml matches the verifier AAR (run `bun run nsc`)", () => {
  const hosts = verifierHosts(aarPath);
  expect(hosts.length).toBeGreaterThan(100);
  expect(hosts).toContain("c.pki.goog");
  expect(readFileSync(xmlPath, "utf8")).toBe(generate(hosts));
});

test("cleartext is limited to loopback and the revocation hosts", () => {
  const xml = generate(["c.pki.goog"]);
  expect(xml).toContain('<base-config cleartextTrafficPermitted="false" />');
  expect(xml).toContain('<domain includeSubdomains="false">127.0.0.1</domain>');
  expect(xml).not.toContain("example.com");
});
