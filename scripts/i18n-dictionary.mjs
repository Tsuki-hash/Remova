import ts from "typescript";

export function dictionaryEntries(text, file) {
  const source = ts.createSourceFile(file, text, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  if (source.parseDiagnostics.length) throw new Error(`${file}: invalid TypeScript dictionary`);
  const declarations = source.statements.filter(ts.isVariableStatement)
    .flatMap(statement => [...statement.declarationList.declarations])
    .filter(declaration => ts.isIdentifier(declaration.name) && declaration.name.text === "dict");
  let object = declarations.length === 1 ? declarations[0].initializer : undefined;
  while (object && (ts.isAsExpression(object) || ts.isSatisfiesExpression(object) || ts.isParenthesizedExpression(object))) object = object.expression;
  if (!object || !ts.isObjectLiteralExpression(object)) throw new Error(`${file}: expected one literal dict`);
  const names = new Set();
  return object.properties.map(property => {
    if (!ts.isPropertyAssignment(property) ||
      !(ts.isIdentifier(property.name) || ts.isStringLiteral(property.name))) throw new Error(`${file}: unsupported dictionary entry`);
    const name = property.name.text;
    if (names.has(name)) throw new Error(`${file}: duplicate dictionary key ${name}`);
    names.add(name);
    const scanner = ts.createScanner(ts.ScriptTarget.Latest, true, ts.LanguageVariant.Standard, text);
    scanner.setTextPos(property.end);
    const end = scanner.scan() === ts.SyntaxKind.CommaToken ? scanner.getTextPos() : property.end;
    return { name, start: property.getFullStart(), end };
  });
}
