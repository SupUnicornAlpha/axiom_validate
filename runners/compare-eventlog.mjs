import fs from "node:fs"

const [actualPath, goldenPath] = process.argv.slice(2)

function normalized(path) {
  return fs
    .readFileSync(path, "utf8")
    .split("\n")
    .filter(Boolean)
    .map((line) => {
      const event = JSON.parse(line)
      event.timestamp_ms = 0
      event.writer_epoch = 1
      const digest = event.spec_digest
      event.spec_digest = "<spec-digest>"
      if (event.commit_id) {
        event.commit_id = event.commit_id.replace(digest, "<spec-digest>")
      }
      return canonicalize(event)
    })
}

function canonicalize(value) {
  if (Array.isArray(value)) return value.map(canonicalize)
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.keys(value)
        .sort()
        .map((key) => [key, canonicalize(value[key])]),
    )
  }
  return value
}

const equal = JSON.stringify(normalized(actualPath)) === JSON.stringify(normalized(goldenPath))
console.log(JSON.stringify({ equal }))
process.exit(equal ? 0 : 1)
