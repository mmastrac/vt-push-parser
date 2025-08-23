# Escapes
## A very large CSI escape
```
<ESC>[?25;1;2;3:4;5;6;7;8;9;10;11;12;13;14;15;16;17;18;19;20;21;22;23;24;25;26;27;28;29;30;31;32;33;34;35;36;37;38;39;40;41;42;43;44;45;46;47;48;49;50;51;52;53;54;55;56;57;58;59;60;61;62;63;64;65;66;67;68;69;70;71;72;73;74;75;76;77;78;79;80;81;82;83;84;85;86;87;88;89;90;91;92;93;94;95;96;97;98;99;100m
```

```
Csi('?', '25', '1', '2', '3:4', '5', '6', '7', '8', '9', '10', '11', '12', '13', '14', '15', '16', '17', '18', '19', '20', '21', '22', '23', '24', '25', '26', '27', '28', '29', '30', '31', '32', '33', '34', '35', '36', '37', '38', '39', '40', '41', '42', '43', '44', '45', '46', '47', '48', '49', '50', '51', '52', '53', '54', '55', '56', '57', '58', '59', '60', '61', '62', '63', '64', '65', '66', '67', '68', '69', '70', '71', '72', '73', '74', '75', '76', '77', '78', '79', '80', '81', '82', '83', '84', '85', '86', '87', '88', '89', '90', '91', '92', '93', '94', '95', '96', '97', '98', '99', '100', '', 'm')
```
---
## Captured from iTerm2
```
<ESC>[ITERM2 3.5.14n<ESC>]10;rgb:dcaa/dcab/dcaa<ESC>\<ESC>]11;rgb:158e/193a/1e75<ESC>\<ESC>[?64;1;2;4;6;17;18;21;22c<ESC>[>64;2500;0c<ESC>P!|6954726D<ESC>\<ESC>P>|iTerm2 3.5.14<ESC>\<ESC>[8;34;148t
```

```
Csi(, '', 'I')
Raw('TERM2 3.5.14n')
OscStart
OscData('10;rgb:dcaa/dcab/dcaa')
OscEnd
OscStart
OscData('11;rgb:158e/193a/1e75')
OscEnd
Csi('?', '64', '1', '2', '4', '6', '17', '18', '21', '22', '', 'c')
Csi('>', '64', '2500', '0', '', 'c')
DcsStart(, '!', |)
DcsData('6954726D')
DcsEnd
DcsStart('>', '', |)
DcsData('iTerm2 3.5.14')
DcsEnd
Csi(, '8', '34', '148', '', 't')
```
---
## No escapes, just control chars
```
<LF><CR><TAB><BS><FF><VT>
```

```
C0(0a)
C0(0d)
C0(09)
C0(08)
C0(0c)
C0(0b)
```
---
## Raw text with line feed control character
```
Hello<LF>World
```

```
Raw('Hello')
C0(0a)
Raw('World')
```
---
## Raw text with delete character (should be ignored)
```
Hello<DEL>World
```

```
Raw('HelloWorld')
```
---
## CSI: Cursor down with parameters 1,2,3
```
<ESC>[1;2;3d
```

```
Csi(, '1', '2', '3', '', 'd')
```
---
## CSI: Show cursor (private parameter ?25, final h)
```
<ESC>[?25h
```

```
Csi('?', '25', '', 'h')
```
---
## CSI: Set graphics mode with multiple parameters (1,2,3,4,5, final m)
```
<ESC>[1;2;3;4;5m
```

```
Csi(, '1', '2', '3', '4', '5', '', 'm')
```
---
## CSI: Set graphics mode with colon parameter (3:1,2,3,4,5, final m)
```
<ESC>[3:1;2;3;4;5m
```

```
Csi(, '3:1', '2', '3', '4', '5', '', 'm')
```
---
## CSI: Cursor up with intermediate space character (final M)
```
<ESC>[  M
```

```
Csi(, '  ', 'M')
```
---
## OSC: Set foreground color to red (10;rgb:fff/000/000) terminated by BEL
```
<ESC>]10;rgb:fff/000/000<BEL>
```

```
OscStart
OscData('10;rgb:fff/000/000')
OscEnd
```
---
## OSC: Set background color to green (11;rgb:000/fff/000) terminated by ST
```
<ESC>]11;rgb:000/fff/000<ESC>\
```

```
OscStart
OscData('11;rgb:000/fff/000')
OscEnd
```
---
## OSC: Set text color (12;test [data) terminated by ST
```
<ESC>]12;test [data<ESC>\
```

```
OscStart
OscData('12;test [data')
OscEnd
```
---
## DCS: Device control string with parameters (1,2,3) and payload terminated by ST
```
<ESC>P 1;2;3|test data<ESC>\
```

```
DcsStart(, ' ', 1)
DcsData(';2;3|test data')
DcsEnd
```
---
## DCS: Device control string with private parameter > and payload terminated by ST
```
<ESC>P>1;2;3|more data<ESC>\
```

```
DcsStart('>', '1', '2', '3', '', |)
DcsData('more data')
DcsEnd
```
---
## DCS: Device control string with intermediate space and payload terminated by ST
```
<ESC>P 1;2;3  |data<ESC>\
```

```
DcsStart(, ' ', 1)
DcsData(';2;3  |data')
DcsEnd
```
---
## DCS: Device control string with final r and payload terminated by ST
```
<ESC>P1$r<ESC>\
```

```
DcsStart(, '1', '$', r)
DcsEnd
```
---
## ESC: Escape sequence with intermediate space and final M
```
<ESC>  M
```

```
Esc('  ', M)
```
---
## SS3: Single shift 3 with final A (arrow key)
```
<ESC>OA
```

```
Esc('', O)
Raw('A')
```
---
## DCS: Device control string with colon parameter (invalid/DCS_IGNORE) cancelled by CAN
```
<ESC>P:1;2;3|data<CAN>Hello
```

```
Raw('Hello')
```
---
## DCS: Device control string with colon parameter (invalid/DCS_IGNORE) cancelled by SUB
```
<ESC>P:1;2;3|data<SUB>Hello
```

```
Raw('Hello')
```
---
## SOS: Start of string (ESC X) with payload terminated by ST
```
<ESC>Xtest data<ESC>\
```

```
```
---
## PM: Privacy message (ESC ^) with payload terminated by ST
```
<ESC>^test data<ESC>\
```

```
```
---
## APC: Application program command (ESC _) with payload terminated by ST
```
<ESC>_test data<ESC>\
```

```
```
---
## CSI: Cursor down sequence cancelled by CAN
```
x<ESC>[1;2;3<CAN>y
```

```
Raw('x')
Raw('y')
```
---
## CSI: Cursor down sequence cancelled by SUB
```
x<ESC>[1;2;3<SUB>y
```

```
Raw('x')
Raw('y')
```
---
## DCS: Device control string cancelled by CAN
```
x<ESC>P 1;2;3|data<CAN>y
```

```
Raw('x')
DcsStart(, ' ', 1)
DcsCancel
Raw('y')
```
---
## OSC: Operating system command cancelled by SUB
```
x<ESC>]10;data<SUB>y
```

```
Raw('x')
OscStart
OscCancel
Raw('y')
```
---
## CSI: Invalid final byte g (should be ignored)
```
x<ESC>[1;2;3gy
```

```
Raw('x')
Csi(, '1', '2', '3', '', 'g')
Raw('y')
```
---
## CSI: Invalid colon parameter (should be ignored)
```
x<ESC>[:1;2;3gy
```

```
Raw('x')
Csi(, ':1', '2', '3', '', 'g')
Raw('y')
```
---
## ESC ESC: Double escape followed by CSI cursor down
```
<ESC><ESC>[1;2;3d
```

```
Csi(, '1', '2', '3', '', 'd')
```
---
## DCS: Device control string with escaped ESC in payload
```
<ESC>P 1;2;3|<ESC><ESC>data<ESC>\
```

```
DcsStart(, ' ', 1)
DcsData(';2;3|<ESC>data')
DcsEnd
```
---
## CSI: Graphics mode with DEL character in parameters
```
<ESC>[1;2;3<DEL>m
```

```
Csi(, '1', '2', '3', '', 'm')
```
---
## DCS: Device control string with colon parameter (invalid) in text context
```
Hello<ESC>P:1;2;3|data<ESC>\World
```

```
Raw('Hello')
Raw('World')
```
---
## DCS: Device control string with colon parameter (invalid) in text context
```
<ESC>P:1;2;3|data<ESC>\Hello
```

```
Raw('Hello')
```
---
## DCS: Valid device control string in text context
```
<ESC>P1;2;3|data<ESC>\Hello
```

```
DcsStart(, '1', '2', '3', '', |)
DcsData('data')
DcsEnd
Raw('Hello')
```
---
## CSI: FG truecolor
```
<ESC>[38:2:255:128:64m
```

```
Csi(, '38:2:255:128:64', '', 'm')
```
---
## CSI: BG truecolor
```
<ESC>[48:2:0:0:0m
```

```
Csi(, '48:2:0:0:0', '', 'm')
```
---
## CSI: FG indexed
```
<ESC>[38:5:208m
```

```
Csi(, '38:5:208', '', 'm')
```
---
## CSI: BG indexed
```
<ESC>[48:5:123m
```

```
Csi(, '48:5:123', '', 'm')
```
---
## CSI: Bold + FG indexed + BG truecolor
```
<ESC>[1;38:5:208;48:2:30:30:30m
```

```
Csi(, '1', '38:5:208', '48:2:30:30:30', '', 'm')
```
---
## CSI: Reset + FG truecolor
```
<ESC>[0;38:2:12:34:56m
```

```
Csi(, '0', '38:2:12:34:56', '', 'm')
```
---
## CSI: Underline color truecolor with empty subparam (::)
```
<ESC>[58:2::186:93:0m
```

```
Csi(, '58:2::186:93:0', '', 'm')
```
---
## CSI: FG truecolor + BG indexed + underline color truecolor
```
<ESC>[38:2:10:20:30;48:5:17;58:2::200:100:0m
```

```
Csi(, '38:2:10:20:30', '48:5:17', '58:2::200:100:0', '', 'm')
```
---
## CSI: Colon params with leading zeros
```
<ESC>[38:2:000:007:042m
```

```
Csi(, '38:2:000:007:042', '', 'm')
```
---
## CSI: Large RGB values
```
<ESC>[38:2:300:300:300m
```

```
Csi(, '38:2:300:300:300', '', 'm')
```
---
## CSI: Trailing semicolon with colon param (empty final param)
```
<ESC>[38:5:15;m
```

```
Csi(, '38:5:15', '', '', 'm')
```
---
## CSI: Only colon param (no numeric params)
```
<ESC>[38:2:1:2:3m
```

```
Csi(, '38:2:1:2:3', '', 'm')
```
---
