import { spawnSync } from 'node:child_process';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(scriptDir, '..');

function usage() {
  console.log(`Usage:
  node scripts/parser-parity.mjs [options] <input.svelte>

Options:
  --format <modern|legacy>     AST format to compare. Default: modern.
  --loose                      Pass loose=true to the vendor Svelte parser.
  --vendor <path>              Path to the Svelte vendor clone. Default: vendors/svelte.
  --oxvelte-only               Only run Oxvelte and print its JSON.
  --vendor-only                Only run vendor Svelte and print its JSON.
  --write-oxvelte <path>       Write Oxvelte JSON to a file.
  --write-vendor <path>        Write vendor JSON to a file.
  --help                       Show this help.

Notes:
  Vendor comparison requires the local Svelte clone's Node dependencies.
  This helper intentionally is not part of cargo test.
`);
}

function parseArgs(argv) {
  const options = {
    format: 'modern',
    loose: false,
    vendor: path.join(repoRoot, 'vendors', 'svelte'),
    oxvelteOnly: false,
    vendorOnly: false,
    writeOxvelte: null,
    writeVendor: null,
    input: null,
  };

  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--help' || arg === '-h') {
      usage();
      process.exit(0);
    } else if (arg === '--format') {
      options.format = argv[++i];
    } else if (arg === '--loose') {
      options.loose = true;
    } else if (arg === '--vendor') {
      options.vendor = path.resolve(argv[++i]);
    } else if (arg === '--oxvelte-only') {
      options.oxvelteOnly = true;
    } else if (arg === '--vendor-only') {
      options.vendorOnly = true;
    } else if (arg === '--write-oxvelte') {
      options.writeOxvelte = path.resolve(argv[++i]);
    } else if (arg === '--write-vendor') {
      options.writeVendor = path.resolve(argv[++i]);
    } else if (arg.startsWith('--')) {
      throw new Error(`Unknown option: ${arg}`);
    } else if (!options.input) {
      options.input = path.resolve(arg);
    } else {
      throw new Error(`Unexpected extra argument: ${arg}`);
    }
  }

  if (!['modern', 'legacy'].includes(options.format)) {
    throw new Error(`Unsupported --format: ${options.format}`);
  }
  if (!options.input) {
    throw new Error('Missing input.svelte path');
  }
  if (options.oxvelteOnly && options.vendorOnly) {
    throw new Error('--oxvelte-only and --vendor-only are mutually exclusive');
  }

  return options;
}

function parseOxvelte(input, format) {
  const result = spawnSync(
    'cargo',
    ['run', '--quiet', '--', 'parse', input, '--format', format],
    {
      cwd: repoRoot,
      encoding: 'utf8',
      maxBuffer: 1024 * 1024 * 64,
    },
  );

  if (result.status !== 0) {
    throw new Error(`Oxvelte parse failed:\n${result.stderr || result.stdout}`);
  }

  try {
    return JSON.parse(result.stdout);
  } catch (error) {
    throw new Error(`Oxvelte output was not valid JSON: ${error.message}\n${result.stdout}`);
  }
}

async function parseVendor(input, options) {
  const compilerPath = path.join(
    options.vendor,
    'packages',
    'svelte',
    'src',
    'compiler',
    'index.js',
  );

  if (!existsSync(compilerPath)) {
    throw new Error(`Cannot find vendor compiler at ${compilerPath}`);
  }

  let compiler;
  try {
    compiler = await import(pathToFileURL(compilerPath).href);
  } catch (error) {
    throw new Error(
      `Could not import vendor Svelte compiler. Install vendor dependencies first.\n${error.message}`,
    );
  }

  const source = readFileSync(input, 'utf8');
  return compiler.parse(source, {
    modern: options.format === 'modern',
    loose: options.loose,
  });
}

function stable(value) {
  if (value === undefined) {
    return undefined;
  }
  if (Array.isArray(value)) {
    return value.map(stable);
  }
  if (value && typeof value === 'object') {
    const result = {};
    for (const key of Object.keys(value).sort()) {
      const stableValue = stable(value[key]);
      if (stableValue !== undefined) {
        result[key] = stableValue;
      }
    }
    return result;
  }
  return value;
}

function jsonDiff(expected, actual, pathName = '$', out = []) {
  if (out.length >= 50) {
    return out;
  }

  if (Array.isArray(expected) && Array.isArray(actual)) {
    if (expected.length !== actual.length) {
      out.push(`${pathName}: array length ${expected.length} vs ${actual.length}`);
    }
    const len = Math.min(expected.length, actual.length);
    for (let i = 0; i < len; i += 1) {
      jsonDiff(expected[i], actual[i], `${pathName}[${i}]`, out);
    }
    return out;
  }

  if (
    expected &&
    actual &&
    typeof expected === 'object' &&
    typeof actual === 'object' &&
    !Array.isArray(expected) &&
    !Array.isArray(actual)
  ) {
    for (const key of Object.keys(expected)) {
      if (!(key in actual)) {
        out.push(`${pathName}.${key}: missing in actual`);
      } else {
        jsonDiff(expected[key], actual[key], `${pathName}.${key}`, out);
      }
    }
    for (const key of Object.keys(actual)) {
      if (!(key in expected)) {
        out.push(`${pathName}.${key}: unexpected in actual`);
      }
    }
    return out;
  }

  if (JSON.stringify(expected) !== JSON.stringify(actual)) {
    out.push(`${pathName}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
  }

  return out;
}

function printJson(value) {
  console.log(JSON.stringify(value, null, 2));
}

async function main() {
  const options = parseArgs(process.argv.slice(2));

  let oxvelteJson = null;
  let vendorJson = null;

  if (!options.vendorOnly) {
    oxvelteJson = parseOxvelte(options.input, options.format);
    if (options.writeOxvelte) {
      writeFileSync(options.writeOxvelte, `${JSON.stringify(oxvelteJson, null, 2)}\n`);
    }
  }

  if (!options.oxvelteOnly) {
    vendorJson = await parseVendor(options.input, options);
    if (options.writeVendor) {
      writeFileSync(options.writeVendor, `${JSON.stringify(vendorJson, null, 2)}\n`);
    }
  }

  if (options.oxvelteOnly) {
    printJson(oxvelteJson);
    return;
  }

  if (options.vendorOnly) {
    printJson(vendorJson);
    return;
  }

  const expected = stable(vendorJson);
  const actual = stable(oxvelteJson);
  const diffs = jsonDiff(expected, actual);
  if (diffs.length > 0) {
    console.error(`Parser parity mismatch (${diffs.length} shown, max 50):`);
    for (const diff of diffs) {
      console.error(`- ${diff}`);
    }
    process.exit(1);
  }

  console.log(`Parser parity OK for ${path.relative(repoRoot, options.input)} (${options.format})`);
}

main().catch((error) => {
  console.error(error.message);
  process.exit(2);
});
