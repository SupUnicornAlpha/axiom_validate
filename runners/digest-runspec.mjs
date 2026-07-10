import crypto from "node:crypto"
import fs from "node:fs"

const [runSpecPath] = process.argv.slice(2)
const runSpec = JSON.parse(fs.readFileSync(runSpecPath, "utf8"))
const digest = crypto.createHash("sha256").update(JSON.stringify(runSpec)).digest("hex")
console.log(digest)
