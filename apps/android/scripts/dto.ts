/** Generates the Kotlin protocol DTOs from the ts-rs output the web family already uses
 * (apps/web/src/core/generated), so a protocol change shows up here as a stale file instead
 * of a runtime decode error. Run with `bun run dto`; a test fails when the committed Kotlin
 * is stale.
 *
 * ts-rs writes a small, regular TypeScript subset: object types, string literal unions,
 * unions of tagged objects, `Array<T>`, `T | null`, two-element tuples and one generic
 * (`ListDiff<T>`, written by hand in ListDiff.kt). ts-rs says `number` for every integer
 * and float, so the numeric kind comes from the field name below; an unknown numeric field
 * stops the generator instead of guessing. */

import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import ts from "typescript";

export const sourceDir = join(import.meta.dir, "../../web/src/core/generated");
export const outPath = join(import.meta.dir, "../core/src/main/kotlin/app/marquee/core/model/Generated.kt");

/** Hand-written in Kotlin: generic or tuple-carrying types the generator does not emit. */
const handWritten = new Set(["ListDiff"]);

const LONG = new Set([
  "resumeMs",
  "leftMs",
  "issuedAtMs",
  "expiresInMs",
  "revision",
  "sizeBytes",
  "maxSizeBytes",
  "addedMs",
  "clockGapMs",
  "rotationWaitMs",
  "expiresMs",
  "startMs",
  "endMs",
  "nextAtMs",
  "attempt",
  "sinceMs",
  "lastOkMs",
]);
const FLOAT = new Set(["progress"]);
const INT = new Set([
  "position",
  "max",
  "epoch",
  "pending",
  "series",
  "pushed",
  "stale",
  "refused",
  "waiting",
  "pulled",
  "applied",
  "unreadable",
  "newer",
  "parked",
  "oldEpochWrites",
  "season",
  "episode",
  "count",
  "seasons",
  "selectedSeason",
  "maxHeight",
  "seeds",
  "failoverCount",
  "titles",
  "inProgress",
  "watched",
  "addonsImported",
  "seriesUnmatched",
  "byReason[]",
  "pendingWrites",
  "unmapped",
  "done",
  "total",
]);

type TypeIr =
  | { k: "string" }
  | { k: "boolean" }
  | { k: "number"; field: string }
  | { k: "array"; of: TypeIr }
  | { k: "nullable"; of: TypeIr }
  | { k: "tuple"; items: TypeIr[] }
  | { k: "ref"; name: string; args: TypeIr[] };

interface Field {
  name: string;
  type: TypeIr;
  doc: string | null;
}

interface Variant {
  tag: string;
  fields: Field[];
}

type Decl =
  | { name: string; doc: string | null; kind: "object"; fields: Field[] }
  | { name: string; doc: string | null; kind: "enum"; values: string[] }
  | { name: string; doc: string | null; kind: "tagged"; tagField: string; variants: Variant[] };

const pascal = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);
const unquote = (s: string) => s.replace(/^["']|["']$/g, "");

function docOf(node: ts.Node): string | null {
  const docs = (node as ts.Node & { jsDoc?: ts.JSDoc[] }).jsDoc;
  const text = docs?.map((d) => (typeof d.comment === "string" ? d.comment : "")).join("\n").trim();
  return text ? text : null;
}

function typeIr(node: ts.TypeNode, field: string): TypeIr {
  if (node.kind === ts.SyntaxKind.StringKeyword) return { k: "string" };
  if (node.kind === ts.SyntaxKind.BooleanKeyword) return { k: "boolean" };
  if (node.kind === ts.SyntaxKind.NumberKeyword) return { k: "number", field };
  // An opaque record the app keeps in its keystore and hands back verbatim (the Trakt record).
  if (node.kind === ts.SyntaxKind.UnknownKeyword) return { k: "nullable", of: { k: "ref", name: "KeystoreRecord", args: [] } };
  if (ts.isUnionTypeNode(node)) {
    const parts = node.types.filter((t) => !(ts.isLiteralTypeNode(t) && t.literal.kind === ts.SyntaxKind.NullKeyword));
    if (parts.length === node.types.length - 1 && parts.length === 1) {
      return { k: "nullable", of: typeIr(parts[0] as ts.TypeNode, field) };
    }
  }
  if (ts.isTupleTypeNode(node)) return { k: "tuple", items: node.elements.map((e) => typeIr(e, `${field}[]`)) };
  if (ts.isTypeReferenceNode(node)) {
    const name = node.typeName.getText();
    // The sync keystore record is opaque to the app: it stores it and hands it back (SyncRecord,
    // a JsonObject, in Sync.kt).
    if (name === "Record" && node.typeArguments?.length === 2) return { k: "ref", name: "SyncRecord", args: [] };
    const args = (node.typeArguments ?? []).map((t) => typeIr(t, field));
    if (name === "Array") return { k: "array", of: args[0] as TypeIr };
    return { k: "ref", name, args };
  }
  throw new Error(`unsupported type in ${field}: ${node.getText()}`);
}

function fieldsOf(literal: ts.TypeLiteralNode, skip?: string): Field[] {
  const out: Field[] = [];
  for (const member of literal.members) {
    if (!ts.isPropertySignature(member) || !member.type) throw new Error(`unsupported member: ${member.getText()}`);
    if (member.questionToken) throw new Error(`optional member: ${member.getText()} (ts-rs writes null, not ?:)`);
    const name = unquote(member.name.getText());
    if (name === skip) continue;
    out.push({ name, type: typeIr(member.type, name), doc: docOf(member) });
  }
  return out;
}

function isStringLiteral(t: ts.TypeNode): t is ts.LiteralTypeNode {
  return ts.isLiteralTypeNode(t) && ts.isStringLiteral(t.literal);
}

function taggedOf(name: string, members: ts.TypeLiteralNode[]): { tagField: string; variants: Variant[] } {
  // The tag is the property that is a string literal in every member.
  const first = members[0] as ts.TypeLiteralNode;
  const tagField = first.members
    .filter(ts.isPropertySignature)
    .map((m) => unquote(m.name.getText()))
    .find((candidate) =>
      members.every((m) =>
        m.members.some(
          (p) => ts.isPropertySignature(p) && unquote(p.name.getText()) === candidate && p.type && isStringLiteral(p.type),
        ),
      ),
    );
  if (!tagField) throw new Error(`${name}: no common tag property`);
  const variants = members.map((m) => {
    const tagMember = m.members.find((p) => ts.isPropertySignature(p) && unquote(p.name.getText()) === tagField);
    const tag = unquote(((tagMember as ts.PropertySignature).type as ts.LiteralTypeNode).literal.getText());
    return { tag, fields: fieldsOf(m, tagField) };
  });
  return { tagField, variants };
}

function declOf(statement: ts.TypeAliasDeclaration): Decl | null {
  const name = statement.name.text;
  if (handWritten.has(name)) return null;
  const doc = docOf(statement);
  const type = statement.type;
  if (ts.isTypeLiteralNode(type)) return { name, doc, kind: "object", fields: fieldsOf(type) };
  if (ts.isUnionTypeNode(type) && type.types.every(isStringLiteral)) {
    return { name, doc, kind: "enum", values: type.types.map((t) => unquote((t as ts.LiteralTypeNode).literal.getText())) };
  }
  if (ts.isUnionTypeNode(type) && type.types.every(ts.isTypeLiteralNode)) {
    return { name, doc, kind: "tagged", ...taggedOf(name, type.types as ts.TypeLiteralNode[]) };
  }
  // serde's flatten of a tagged enum into a struct: `{common} & ({tag: "a", ...} | ...)`. One class
  // with the tag as text and every variant's fields, null where the variant has none.
  if (ts.isIntersectionTypeNode(type) && type.types.length === 2) {
    const [common, rest] = type.types as [ts.TypeNode, ts.TypeNode];
    const union = ts.isParenthesizedTypeNode(rest) ? rest.type : rest;
    if (ts.isTypeLiteralNode(common) && ts.isUnionTypeNode(union) && union.types.every(ts.isTypeLiteralNode)) {
      const { tagField, variants } = taggedOf(name, union.types as ts.TypeLiteralNode[]);
      const fields: Field[] = [{ name: tagField, type: { k: "string" }, doc: null }];
      for (const f of variants.flatMap((v) => v.fields)) {
        const nullable: TypeIr = f.type.k === "nullable" ? f.type : { k: "nullable", of: f.type };
        const seen = fields.find((g) => g.name === f.name);
        if (seen && JSON.stringify(seen.type) !== JSON.stringify(nullable)) throw new Error(`${name}.${f.name}: variants disagree`);
        if (!seen) fields.push({ ...f, type: nullable });
      }
      return { name, doc, kind: "object", fields: [...fields, ...fieldsOf(common)] };
    }
  }
  throw new Error(`${name}: unsupported declaration`);
}

function numberType(field: string): string {
  if (LONG.has(field)) return "Long";
  if (FLOAT.has(field)) return "Float";
  if (INT.has(field)) return "Int";
  throw new Error(`numeric field '${field}' has no Kotlin type: add it to LONG, FLOAT or INT in scripts/dto.ts`);
}

function kotlinType(t: TypeIr): string {
  switch (t.k) {
    case "string":
      return "String";
    case "boolean":
      return "Boolean";
    case "number":
      return numberType(t.field);
    case "array":
      return `List<${kotlinType(t.of)}>`;
    case "nullable":
      return `${kotlinType(t.of)}?`;
    case "tuple":
      if (t.items.length !== 2) throw new Error("only two-element tuples are supported");
      return `Tuple2<${t.items.map(kotlinType).join(", ")}>`;
    case "ref":
      return t.args.length ? `${t.name}<${t.args.map(kotlinType).join(", ")}>` : t.name;
  }
}

const kdoc = (doc: string | null, indent: string) =>
  doc ? `${indent}/** ${doc.replace(/\s*\n\s*/g, " ")} */\n` : "";

function params(fields: Field[], indent: string): string {
  return fields
    .map((f) => {
      const type = kotlinType(f.type);
      // A field the core leaves out (an older recording, a newer field) is null or empty.
      const fallback = f.type.k === "nullable" ? " = null" : f.type.k === "array" ? " = emptyList()" : "";
      return `${kdoc(f.doc, indent)}${indent}val ${f.name}: ${type}${fallback},`;
    })
    .join("\n");
}

/** Tagged unions whose string fields are keys, tokens or secrets: printed as hidden. */
const SECRET_UNIONS = new Set(["DebridCredential"]);

const isRecord = (t: TypeIr): boolean =>
  (t.k === "ref" && (t.name === "SyncRecord" || t.name === "KeystoreRecord")) || (t.k === "nullable" && isRecord(t.of));

/**
 * A class that carries the keystore record gets a `toString` that never prints it (it holds keys
 * and a token), whatever logs or crash reports a value ends up in. Other fields print as usual.
 */
function redactingToString(className: string, fields: Field[], indent: string, secret = false): string | null {
  const hidden = (f: Field) => isRecord(f.type) || (secret && f.type.k === "string");
  if (!fields.some(hidden)) return null;
  const parts = fields.map((f) => {
    if (!hidden(f)) return `${f.name}=$${f.name}`;
    return f.type.k === "nullable" ? `${f.name}=\${${f.name}?.let { "hidden" }}` : `${f.name}=hidden`;
  });
  return (
    `${indent}// Holds keys or a token: never printed.\n` +
    `${indent}override fun toString() = "${className}(${parts.join(", ")})"`
  );
}

function emit(d: Decl): string {
  switch (d.kind) {
    case "object": {
      const body = redactingToString(d.name, d.fields, "    ");
      return `${kdoc(d.doc, "")}@Serializable\ndata class ${d.name}(\n${params(d.fields, "    ")}\n)${body ? ` {\n${body}\n}` : ""}\n`;
    }
    case "enum": {
      const entries = d.values.map((v) => `    @SerialName("${v}") ${pascal(v)},`).join("\n");
      return `${kdoc(d.doc, "")}@Serializable\nenum class ${d.name} {\n${entries}\n}\n`;
    }
    case "tagged": {
      const body = d.variants
        .map((v) => {
          const cls = pascal(v.tag);
          if (v.fields.length === 0) {
            return `    @Serializable\n    @SerialName("${v.tag}")\n    data object ${cls} : ${d.name}`;
          }
          const body = redactingToString(cls, v.fields, "        ", SECRET_UNIONS.has(d.name));
          return `    @Serializable\n    @SerialName("${v.tag}")\n    data class ${cls}(\n${params(v.fields, "        ")}\n    ) : ${d.name}${body ? ` {\n${body}\n    }` : ""}`;
        })
        .join("\n\n");
      return `${kdoc(d.doc, "")}@Serializable\n@JsonClassDiscriminator("${d.tagField}")\nsealed interface ${d.name} {\n${body}\n}\n`;
    }
  }
}

export function generate(dir: string): string {
  const decls: Decl[] = [];
  for (const file of readdirSync(dir).filter((f) => f.endsWith(".ts")).sort()) {
    const source = ts.createSourceFile(file, readFileSync(join(dir, file), "utf8"), ts.ScriptTarget.ES2022, true);
    for (const statement of source.statements) {
      if (ts.isTypeAliasDeclaration(statement)) {
        const decl = declOf(statement);
        if (decl) decls.push(decl);
      }
    }
  }
  decls.sort((a, b) => a.name.localeCompare(b.name));
  return [
    "// Generated by scripts/dto.ts from apps/web/src/core/generated (ts-rs). Do not edit.",
    "@file:OptIn(kotlinx.serialization.ExperimentalSerializationApi::class)",
    "",
    "package app.marquee.core.model",
    "",
    "import kotlinx.serialization.SerialName",
    "import kotlinx.serialization.Serializable",
    "import kotlinx.serialization.json.JsonClassDiscriminator",
    "",
    decls.map(emit).join("\n"),
  ].join("\n");
}

if (import.meta.main) {
  writeFileSync(outPath, generate(sourceDir));
  console.log(`wrote ${outPath}`);
}
