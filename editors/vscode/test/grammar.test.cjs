const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { test, before, after } = require('node:test');
const { Registry, parseRawGrammar, INITIAL } = require('vscode-textmate');
const { loadWASM, createOnigScanner, createOnigString } = require('vscode-oniguruma');

const root = path.resolve(__dirname, '..');
const manifest = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8'));
const contribution = manifest.contributes.grammars[0];
let registry;
let grammar;
before(async () => {
    const wasm = fs.readFileSync(require.resolve('vscode-oniguruma/release/onig.wasm'));
    await loadWASM(wasm.buffer.slice(wasm.byteOffset, wasm.byteOffset + wasm.byteLength));
    registry = new Registry({
        onigLib: Promise.resolve({ createOnigScanner, createOnigString }),
        loadGrammar: async (scope) => scope === contribution.scopeName
            ? parseRawGrammar(fs.readFileSync(path.join(root, contribution.path), 'utf8'), contribution.path)
            : null,
    });
    grammar = await registry.loadGrammar(contribution.scopeName);
});
after(() => registry?.dispose());

function tokenize(source) {
    let state = INITIAL;
    return source.split('\n').map((line) => {
        const result = grammar.tokenizeLine(line, state);
        state = result.ruleStack;
        return result.tokens.map(({ startIndex, endIndex, scopes }) => ({
            text: line.slice(startIndex, endIndex), scopes,
        }));
    });
}
function has(tokens, text, scope) {
    assert.ok(tokens.some((token) => token.text === text && token.scopes.includes(scope)),
        `Expected ${JSON.stringify(text)} in ${scope}: ${JSON.stringify(tokens)}`);
}
function line(source) { return tokenize(source)[0]; }

test('manifest associates .spk with the loaded grammar and basic editing configuration', () => {
    const language = manifest.contributes.languages[0];
    assert.equal(language.id, contribution.language);
    assert.deepEqual(language.extensions, ['.spk']);
    assert.equal(line('game')[0].scopes[0], contribution.scopeName);
    assert.equal(manifest.main, undefined);
    const config = JSON.parse(fs.readFileSync(path.join(root, language.configuration), 'utf8'));
    assert.equal(config.comments.lineComment, '//');
    assert.equal(config.comments.blockComment, undefined);
    assert.deepEqual(config.brackets, [['{', '}'], ['[', ']'], ['(', ')']]);
    assert.ok(config.autoClosingPairs.every(pair => pair.notIn.includes('comment') && pair.notIn.includes('string')));
});

test('all compiler keywords and built-in names stay covered', () => {
    const lexer = fs.readFileSync(path.join(root, '../../src/lexer.rs'), 'utf8');
    const keywords = [...lexer.matchAll(/"([a-z0-9_]+)" => TokenKind::/g)].map(match => match[1]);
    assert.ok(keywords.includes('game') && keywords.includes('void'));
    for (const keyword of keywords) {
        assert.ok(line(keyword).some(token => token.scopes.some(scope => /^(keyword|storage|constant)\./.test(scope))), keyword);
        for (const identifier of [`${keyword}_suffix`, `prefix_${keyword}`, `${keyword}2`]) {
            has(line(identifier), identifier, 'variable.other.speck');
        }
    }
    const builtins = fs.readFileSync(path.join(root, '../../src/builtins.rs'), 'utf8');
    const names = [...builtins.matchAll(/name: "([A-Za-z0-9_]+)"/g)].map(match => match[1]);
    assert.ok(names.length >= 19);
    for (const name of names) {
        has(line(name.startsWith('KEY_') ? name : `${name}()`), name,
            name.startsWith('KEY_') ? 'support.constant.speck' : 'support.function.speck');
        has(line(`${name}_suffix`), `${name}_suffix`, 'variable.other.speck');
    }
});

test('Unicode titles and comments isolate keywords, quotes, and operators', () => {
    const tokens = tokenize('game "Böots 🥾 // if 3.5" // "fn" KEY_W\nlet x: i32 = 2');
    has(tokens[0], 'Böots 🥾 // if 3.5', 'string.quoted.double.speck');
    has(tokens[0], '// "fn" KEY_W', 'comment.line.double-slash.speck');
    has(tokens[1], 'let', 'storage.modifier.speck');
});

test('titles use the lexer’s raw backslashes, with no escape or multiline strings', () => {
    const raw = line(String.raw`game "Boots\n\t"`);
    has(raw, String.raw`Boots\n\t`, 'string.quoted.double.speck');
    assert.ok(raw.every(token => token.scopes.every(scope => !scope.includes('escape'))));
    // A backslash does not escape the closing quote in the Speck lexer.
    const quote = line(String.raw`game "Boots\" // comment`);
    has(quote, 'Boots\\', 'string.quoted.double.speck');
    has(quote, '// comment', 'comment.line.double-slash.speck');
    const unfinished = tokenize('game "unfinished\nlet x: i32 = 1');
    has(unfinished[1], 'let', 'storage.modifier.speck');
    assert.ok(unfinished[1].every(token => !token.scopes.includes('string.quoted.double.speck')));
});

test('decimal numbers stop at ranges and unsupported exponent syntax', () => {
    const tokens = line('0..8 -450.0 1.25 1e3 2.5E-2 0xFF .5 12.');
    for (const number of ['0', '8', '450.0', '1.25', '1', '2.5', '2', '5', '12']) {
        has(tokens, number, 'constant.numeric.speck');
    }
    has(tokens, '..', 'keyword.operator.speck');
    assert.ok(tokens.every(token => !['1e3', '2.5E-2', '0xFF', '.5', '12.'].includes(token.text)));
    has(line('value12'), 'value12', 'variable.other.speck');
});

test('longest-match operators, punctuation and boolean values', () => {
    for (const operator of ['+=', '-=', '*=', '/=', '%=', '<=', '>=', '==', '!=', '&&', '||', '->', '..', '::', '+', '-', '*', '/', '%', '!', '=', '<', '>']) {
        has(line(operator), operator, 'keyword.operator.speck');
    }
    has(line('true'), 'true', 'constant.language.boolean.speck');
    has(line('false'), 'false', 'constant.language.boolean.speck');
    has(line('x.y'), '.', 'punctuation.separator.speck');
});

test('function and type declarations survive comments and multiline signatures', () => {
    const tokens = tokenize('fn move(\n    p: platform, // Player position\n    dt: f32\n) -> platform {\n    return p\n}');
    has(tokens[0], 'move', 'entity.name.function.speck');
    has(tokens[1], 'p', 'variable.parameter.speck');
    has(tokens[1], 'platform', 'entity.name.type.speck');
    has(tokens[2], 'f32', 'storage.type.speck');
    has(tokens[3], 'platform', 'entity.name.type.speck');
    has(tokens[4], 'return', 'keyword.control.speck');
    has(line('move(player)'), 'move', 'entity.name.function.speck');
    const struct = tokenize('struct platform {\n positions: [[f32; 2]; COUNT]\n}\nlet current: platform = old');
    has(struct[0], 'platform', 'entity.name.type.speck');
    has(struct[1], 'positions', 'variable.other.property.speck');
    has(struct[1], 'f32', 'storage.type.speck');
    has(struct[1], 'COUNT', 'variable.other.speck');
    has(struct[3], 'platform', 'entity.name.type.speck');
    has(struct[3], 'old', 'variable.other.speck');
});

test('module keywords and qualified paths in types, array sizes and calls', () => {
    const tokens = tokenize('import "rooms.spk" as rooms\nlet level: [rooms::Room; rooms::COUNT] = rooms::make()\nfn enter(room: rooms::Room) -> rooms::Room { return room }');
    has(tokens[0], 'import', 'keyword.control.speck');
    has(tokens[0], 'as', 'keyword.control.speck');
    has(tokens[1], 'rooms', 'entity.name.namespace.speck');
    has(tokens[1], 'Room', 'entity.name.type.speck');
    has(tokens[1], 'COUNT', 'variable.other.speck');
    assert.equal(tokens[1].filter(token => token.text === '::' && token.scopes.includes('keyword.operator.speck')).length, 3);
    has(tokens[1], 'make', 'entity.name.function.speck');
    has(tokens[2], 'room', 'variable.parameter.speck');
    has(tokens[2], 'rooms', 'entity.name.namespace.speck');
    has(tokens[2], 'Room', 'entity.name.type.speck');
});

test('representative BOOTS source retains useful scopes through nested aggregate data', () => {
    const source = fs.readFileSync(path.join(__dirname, 'fixtures/boots.spk'), 'utf8');
    const tokens = tokenize(source).flat();
    for (const [text, scope] of [
        ['Boots Concept', 'string.quoted.double.speck'], ['Player', 'entity.name.type.speck'],
        ['MAX_PLATFORM_COUNT', 'variable.other.speck'], ['1000.0', 'constant.numeric.speck'],
        ['draw_platform', 'entity.name.function.speck'], ['platform', 'variable.parameter.speck'],
        ['fill_rect', 'support.function.speck'], ['KEY_SPACE', 'support.constant.speck'],
        ['+=', 'keyword.operator.speck'], ['for', 'keyword.control.speck'], ['..', 'keyword.operator.speck'],
    ]) has(tokens, text, scope);
    const lastCall = source.split('\n').findIndex(line => line.includes('draw_platform(level_platforms'));
    has(tokenize(source)[lastCall], 'draw_platform', 'entity.name.function.speck');
    has(tokenize(source)[lastCall], 'level_platforms', 'variable.other.speck');
});
