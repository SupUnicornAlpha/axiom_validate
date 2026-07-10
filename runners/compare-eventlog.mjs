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
      const digest = event.spec_digest
      event.spec_digest = "<spec-digest>"
      if (event.commit_id) {
        event.commit_id = event.commit_id.replace(digest, "<spec-digest>")
      }
      return event
    })
}

const equal = JSON.stringify(normalized(actualPath)) === JSON.stringify(normalized(goldenPath))
console.log(JSON.stringify({ equal }))
process.exit(equal ? 0 : 1)
