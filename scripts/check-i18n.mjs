import { readdir } from 'node:fs/promises';
import assert from 'node:assert/strict';

const source = new URL('../src/', import.meta.url);
const locales = ['en', 'ja', 'zh-CN'];
const files = (await readdir(source)).filter(name => /^(?:.+-)?i18n\.ts$/.test(name)).sort();
const errors = [];
let dictionaries = 0;

function kind(value) {
  if (Array.isArray(value)) return 'array';
  return value === null ? 'null' : typeof value;
}

function compare(expected, actual, path) {
  const type = kind(expected);
  if (type !== kind(actual)) {
    errors.push(`${path}: expected ${type}, got ${kind(actual)}`);
  } else if (type === 'array' || type === 'object') {
    if (type === 'array' && expected.length !== actual.length) {
      errors.push(`${path}: expected ${expected.length} entries, got ${actual.length}`);
    }
    for (const key of Object.keys(expected)) {
      if (!Object.hasOwn(actual, key)) errors.push(`${path}.${key}: missing key`);
      else compare(expected[key], actual[key], `${path}.${key}`);
    }
    for (const key of Object.keys(actual)) {
      if (!Object.hasOwn(expected, key)) errors.push(`${path}.${key}: unexpected key`);
    }
  } else if (type === 'string') {
    if (!actual.trim()) errors.push(`${path}: empty translation`);
    const placeholders = text => (text.match(/\{[a-zA-Z_][a-zA-Z_\d]*\}/g) ?? []).sort().join(' ');
    if (placeholders(expected) !== placeholders(actual)) {
      errors.push(`${path}: placeholders differ from English`);
    }
  }
}

for (const file of files) {
  const exports = await import(new URL(file, source));
  let found = false;
  for (const [name, dictionary] of Object.entries(exports)) {
    if (kind(dictionary) !== 'object') continue;
    found = true;
    dictionaries++;
    const path = `${file}:${name}`;
    for (const locale of locales) {
      if (!Object.hasOwn(dictionary, locale) || kind(dictionary[locale]) !== 'object') {
        errors.push(`${path}.${locale}: missing locale dictionary`);
      } else if (kind(dictionary.en) === 'object') {
        compare(dictionary.en, dictionary[locale], `${path}.${locale}`);
      }
    }
    for (const locale of Object.keys(dictionary)) {
      if (!locales.includes(locale)) errors.push(`${path}.${locale}: unexpected locale`);
    }
  }
  if (!found) errors.push(`${file}: no exported locale dictionary`);
}

assert.ok(files.length, 'No translation files found');
assert.equal(errors.length, 0, errors.join('\n'));
console.log(`Checked ${dictionaries} dictionaries in ${files.length} files for ${locales.join(', ')}.`);
