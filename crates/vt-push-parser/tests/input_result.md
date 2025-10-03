# Input Escapes
## ESC space (Alt+space)
```
<ESC><SP>
```

```
Esc('',  )
```
---
## ESC ! (Alt+!)
```
<ESC>!
```

```
Esc('', !)
```
---
## ESC ( (Alt+()
```
<ESC>(
```

```
Esc('', ()
```
---
## ESC / (Alt+/, last intermediate character)
```
<ESC>/
```

```
Esc('', /)
```
---
## ESC ? (Alt+?)
```
<ESC>?
```

```
Esc('', ?)
```
---
## ESC > (Alt+>)
```
<ESC>>
```

```
Esc('', >)
```
---
## ESC 1 (Alt+1)
```
<ESC>1
```

```
Esc('', 1)
```
---
## ESC M (Alt+M)
```
<ESC>M
```

```
Esc('', M)
```
---
## ESC N (Alt+N, NOT SS2 in input mode)
```
<ESC>N
```

```
Esc('', N)
```
---
## ESC a (Alt+a)
```
<ESC>a
```

```
Esc('', a)
```
---
## Alt+Enter (ESC \r)
```
<ESC><CR>
```

```
Esc('', <CR>)
```
---
## Alt+Tab (ESC \t)
```
<ESC><TAB>
```

```
Esc('', <TAB>)
```
---
## Alt+Newline (ESC \n)
```
<ESC><LF>
```

```
Esc('', <LF>)
```
---
## ESC [ A (Up arrow - CSI sequence)
```
<ESC>[A
```

```
Csi(, '', 'A')
```
---
## ESC O P (F1 - SS3 sequence)
```
<ESC>OP
```

```
Ss3('P')
```
---
## ESC ESC (Alt+ESC)
```
<ESC><ESC>
```

```
Esc('', <ESC>)
```
---
## ESC ESC [A (Alt+ESC followed by raw text [A)
```
<ESC><ESC>[A
```

```
Esc('', <ESC>)
[A
```
---
## ESC CAN (Alt+Ctrl+x)
```
<ESC><CAN>
```

```
Esc('', <CAN>)
```
---
## ESC SUB (Alt+Ctrl+z)
```
<ESC><SUB>
```

```
Esc('', <SUB>)
```
---
## ESC DEL (Alt+Backspace)
```
<ESC><DEL>
```

```
Esc('', <DEL>)
```
---
## Multiple Alt+key sequences in a row
```
<ESC>a<ESC>b<ESC>c
```

```
Esc('', a)
Esc('', b)
Esc('', c)
```
---
## Alt+key mixed with regular text
```
hello<ESC>aworld
```

```
hello
Esc('', a)
world
```
---
## ESC followed by various C0 controls
```
<ESC><NUL><ESC><SOH><ESC><STX>
```

```
Esc('', <NUL>)
Esc('', <SOH>)
Esc('', <STX>)
```
---
## Bracketed paste mode start
```
<ESC>[200~
```

```
PasteStart
```
---
## Bracketed paste mode end
```
<ESC>[201~
```

```
PasteEnd
```
---
## Bracketed paste with simple text
```
<ESC>[200~hello world<ESC>[201~
```

```
PasteStart
hello world
PasteEnd
```
---
## Bracketed paste with newlines
```
<ESC>[200~line1<LF>line2<LF>line3<ESC>[201~
```

```
PasteStart
line1<LF>line2<LF>line3
PasteEnd
```
---
## Bracketed paste with special characters
```
<ESC>[200~!@#$%^&*()<ESC>[201~
```

```
PasteStart
!@#$%^&*()
PasteEnd
```
---
## Bracketed paste with control characters
```
<ESC>[200~<TAB>text<CR><LF>more<ESC>[201~
```

```
PasteStart
<TAB>text<CR><LF>more
PasteEnd
```
---
## Empty bracketed paste
```
<ESC>[200~<ESC>[201~
```

```
PasteStart
PasteEnd
```
---
## Bracketed paste with only whitespace
```
<ESC>[200~   <TAB>  <LF><ESC>[201~
```

```
PasteStart
   <TAB>  <LF>
PasteEnd
```
---
## Multiple bracketed pastes in sequence
```
<ESC>[200~first<ESC>[201~<ESC>[200~second<ESC>[201~
```

```
PasteStart
first
PasteEnd
PasteStart
second
PasteEnd
```
---
