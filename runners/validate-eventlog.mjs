import fs from "node:fs"

const [eventlogPath] = process.argv.slice(2)
const lines = fs
  .readFileSync(eventlogPath, "utf8")
  .split("\n")
  .filter(Boolean)

let validLines = 0
let invalidLines = 0
const lastSequenceByRun = new Map()

for (const line of lines) {
  try {
    const parsed = JSON.parse(line)
    const previousSequence = lastSequenceByRun.get(parsed.run_id) ?? 0
    const valid =
      parsed.schema_version === 1 &&
      parsed.run_id &&
      typeof parsed.spec_digest === "string" &&
      parsed.spec_digest.length === 64 &&
      Number.isInteger(parsed.sequence) &&
      parsed.sequence === previousSequence + 1 &&
      Number.isInteger(parsed.timestamp_ms) &&
      parsed.timestamp_ms >= 0 &&
      Number.isInteger(parsed.writer_epoch) &&
      parsed.writer_epoch > 0 &&
      "kind" in parsed &&
      "detail" in parsed

    if (valid) {
      validLines += 1
      lastSequenceByRun.set(parsed.run_id, parsed.sequence)
    } else {
      invalidLines += 1
    }
  } catch {
    invalidLines += 1
  }
}

console.log(JSON.stringify({ validLines, invalidLines }))
