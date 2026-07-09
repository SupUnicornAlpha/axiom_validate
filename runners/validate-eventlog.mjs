import fs from "node:fs"

const [eventlogPath] = process.argv.slice(2)
const lines = fs
  .readFileSync(eventlogPath, "utf8")
  .split("\n")
  .filter(Boolean)

let validLines = 0
let invalidLines = 0

for (const line of lines) {
  try {
    const parsed = JSON.parse(line)
    if (parsed.run_id && "kind" in parsed && "detail" in parsed) {
      validLines += 1
    } else {
      invalidLines += 1
    }
  } catch {
    invalidLines += 1
  }
}

console.log(JSON.stringify({ validLines, invalidLines }))
