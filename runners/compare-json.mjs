import fs from "node:fs"

function sortValue(value) {
  if (Array.isArray(value)) {
    return value.map(sortValue)
  }
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.keys(value)
        .sort()
        .map((key) => [key, sortValue(value[key])]),
    )
  }
  return value
}

const [generatedPath, fixturePath] = process.argv.slice(2)
const generated = JSON.parse(fs.readFileSync(generatedPath, "utf8"))
const fixture = JSON.parse(fs.readFileSync(fixturePath, "utf8"))

const generatedCanonical = JSON.stringify(sortValue(generated))
const fixtureCanonical = JSON.stringify(sortValue(fixture))

console.log(
  JSON.stringify({
    equal: generatedCanonical === fixtureCanonical,
    generatedLength: generatedCanonical.length,
    fixtureLength: fixtureCanonical.length,
  }),
)
