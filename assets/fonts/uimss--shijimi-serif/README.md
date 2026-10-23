# ShijimiSerif

<img src="https://github.com/uimss/shijimi_serif/blob/main/preview/preview_1.png?raw=true" width="30%"> <img src="https://github.com/uimss/shijimi_serif/blob/main/preview/preview_2.png?raw=true" width="30%"> <img src="https://github.com/uimss/shijimi_serif/blob/main/preview/preview_3.png?raw=true" width="30%">

**ShijimiSerif** is a proportional pixel serif font that contains Latin letters, hiragana, and katakana. 
It was created to be used together with fonts such as **Misaki Mincho**, which are built on a 7px-high grid.

ShijimiSerif adopts only partial serif features and uses a narrower character width, helping prevent text from becoming much longer in localization from Japanese to English or other languages.

For hiragana and katakana, the base is **Misaki Mincho**, but the upward slant impression is toned down for a more balanced look.

- **ShijimiSerif**: Includes Latin letters, hiragana, katakana, and some symbols.  
- **ShijimiSerifJa**: A composite font of ShijimiSerif and Misaki Mincho. Since it supports commonly used kanji and symbols in Japanese, this version is recommended for general use.

## License

**OFL (SIL Open Font License 1.1)**  
[https://licenses.opensource.jp/OFL-1.1/OFL-1.1.html](https://licenses.opensource.jp/OFL-1.1/OFL-1.1.html)  

Attribution is appreciated but not strictly required.

## Supported Languages

- **ShijimiSerif**: English, French, Italian, German, Spanish, Portuguese, Danish, Swedish, Norwegian, Icelandic, Hungarian, Polish, Turkish, Russian, Ukrainian, etc.  
- **ShijimiSerifJa**: All of the above + Japanese  

## File Formats

- **ShijimiSerif**: BDF, TTF  
- **ShijimiSerifJa**: TTF  

## Notes

- The total character height is set to **10px**. In Unity or other environments where pixel-perfect rendering is desired, please use line heights in multiples of 10. (This accommodates descenders such as “g” that extend below the baseline.)  
- In languages with diacritics like “À,” some characters extend 1–2px beyond the 10px frame. This may cause overlap depending on line spacing. For multilingual use, consider increasing line spacing.  

## Special Thanks

- **Misaki Font** (Misaki Mincho was used)  
  [https://littlelimit.net/misaki.htm](https://littlelimit.net/misaki.htm)  
- **Galmuri** (used as a reference for basic glyph shapes)  
  [https://github.com/quiple/galmuri](https://github.com/quiple/galmuri)  

## Changelog

- **2025.09.28**: v2.0 – Added hiragana and katakana  
- **2025.03.05**: v1.1 – Adjusted widths of characters related to “j”  
- **2025.03.04**: v1.0 – Initial release  
