#!/usr/bin/env python3
"""Requires Pygments. Delimiter/lexer inspection, not compilation or Rust parsing."""
from pathlib import Path
from pygments import lex
from pygments.lexers import RustLexer
from pygments.token import Token
root=Path(__file__).resolve().parents[1]
issues=[]
for p in [*root.glob('src/**/*.rs'),*root.glob('tests/**/*.rs'),root/'build.rs']:
    stack=[];line=1
    for kind,text in lex(p.read_text(), RustLexer()):
        if kind in Token.Punctuation:
            for ch in text:
                if ch in '([{': stack.append((ch,line))
                elif ch in ')]}':
                    if not stack or stack[-1][0] != {')':'(',']':'[','}':'{'}[ch]:
                        issues.append(f'{p.relative_to(root)}:{line}: unexpected {ch}, stack={stack[-4:]}')
                        break
                    stack.pop()
        if kind in Token.Error:
            issues.append(f'{p.relative_to(root)}:{line}: lexical error {text!r}')
        line+=text.count('\n')
    if stack: issues.append(f'{p.relative_to(root)}: unclosed {stack[-5:]}')
print('\n'.join(issues) or 'All Rust delimiters balanced; no Pygments lexical errors. NOT a type check or compilation.')

raise SystemExit(1 if issues else 0)
