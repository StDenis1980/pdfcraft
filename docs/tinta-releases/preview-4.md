# Tinta PDF — Preview 4

> **Status:** 2026-11-17 · draft for the Preview 4 release (10 December 2026), version 0.21.1, with
> the 16 November upstream sync · **Revision history:** 2026-11-16 created (changes from Preview 3,
> 12 November, to #58); 2026-11-17 added the Tinta Office look (#60). Updated at each weekly sync until the freeze on 7 December.

Release notes for the GitHub release and the site, in Spanish, Ukrainian and English. Changes since
Preview 3 (v0.18.1, 12 November 2026). Before publishing: build the Windows installers of all five
apps with `packaging/windows/tinta-build-release.ps1` (MSI, portable zip, SHA256SUMS.txt), and run
the smoke test on the installers (`packaging/windows/tinta-smoke.ps1`, `packaging/linux/tinta-smoke.sh`).

## Español

**Tinta PDF — vista previa 4.** Versión de prueba: úsela con copias de sus documentos.

Novedades:

- **Seleccionar texto de varias páginas** de una vez, para copiarlo seguido.
- **Imprimir:** la lista de papel muestra los tamaños propios de su impresora, y las páginas con
  transparencias se imprimen bien también con controladores de impresora antiguos.
- **Formularios con guiones (JavaScript):** se ejecutan las acciones al abrir, guardar e imprimir, las
  de cada página y sus temporizadores (por ejemplo, las animaciones hechas con LaTeX); un enlace con
  JavaScript funciona, y si quiere abrir otro documento se lo pregunta antes.
- **Editar texto:** al sustituir una línea se conserva su tipo de letra siempre que tenga los caracteres.
- **Windows:** el instalador deja elegir la carpeta de instalación, y el menú del Explorador ofrece
  combinar los archivos seleccionados en un PDF.
- **Asistentes y automatización:** cada herramienta indica qué puede hacer (leer o cambiar el
  documento, leer o escribir en el disco, imprimir, firmar…), una sesión puede limitarse a ciertos
  comandos y admite varias carpetas permitidas.
- **Aspecto de Tinta Office:** los colores y la forma de los controles de Tinta Correo (gris suave
  con un toque rojo, esquinas más redondas), en claro y oscuro; una opción en Preferencias › Apariencia vuelve al aspecto clásico.
- Del proyecto original (PdfCraft): muchas correcciones de formularios, firmas, impresión y modelos 3D.

Limitaciones conocidas: la conversión de Word, Excel y PowerPoint depende de Tinta Texto, Calculo y
Presenta (todavía en vista previa); el OCR no lee bien sellos oscuros o de color sobre el texto ni
fondos de guilloche; no hay firma con DNIe ni tarjeta todavía.

## Українська

**Tinta PDF — попередня версія 4.** Тестова версія: працюйте з копіями документів.

Що нового:

- **Виділення тексту на кількох сторінках** одразу, щоб скопіювати його суцільно.
- **Друк:** у списку паперу — власні розміри вашого принтера, а сторінки з прозорістю правильно
  друкуються і на старих драйверах принтерів.
- **Форми зі скриптами (JavaScript):** виконуються дії під час відкриття, збереження і друку, дії
  сторінок і їхні таймери (наприклад, анімації, зроблені в LaTeX); посилання з JavaScript працює, а
  якщо воно хоче відкрити інший документ, програма спершу спитає.
- **Редагування тексту:** заміна рядка зберігає його шрифт, якщо в ньому є потрібні символи.
- **Windows:** інсталятор дає вибрати папку встановлення, а меню Провідника пропонує об'єднати
  вибрані файли в один PDF.
- **Асистенти й автоматизація:** кожен інструмент повідомляє, що він може робити (читати чи змінювати
  документ, читати чи писати на диск, друкувати, підписувати…); сеанс можна обмежити певними
  командами, і він приймає кілька дозволених папок.
- **Вигляд Tinta Office:** кольори й форма елементів як у Tinta Correo (м'який сірий з червоним
  відтінком, заокругленіші кути), у світлій і темній темі; перемикач у налаштуваннях (Вигляд) повертає класичний вигляд.
- З основного проєкту (PdfCraft): багато виправлень форм, підписів, друку і 3D-моделей.

Відомі обмеження: перетворення Word, Excel і PowerPoint залежить від Tinta Texto, Calculo й
Presenta (теж попередні версії); OCR погано читає темні чи кольорові штампи поверх тексту та
гільйошний фон; підпису DNIe чи карткою ще немає.

## English

**Tinta PDF — Preview 4.** A test version: work on copies of your documents.

What's new:

- **Select text across pages** in one go, to copy it as one piece.
- **Printing:** the Paper list shows your printer's own sizes, and pages with transparency print
  correctly on older printer drivers too.
- **Forms with scripts (JavaScript):** open, save and print actions run, as do page actions and their
  timers (animations made with LaTeX, for example); a JavaScript link works, and one that wants to
  open another document asks first.
- **Editing text:** replacing a line keeps its font wherever the font has the characters.
- **Windows:** the installer lets you choose the install folder, and the Explorer menu offers to
  combine the selected files into one PDF.
- **Assistants and automation:** every tool says what it may do (read or change the document, read
  or write files, print, sign…); a session can be limited to some commands and takes several
  allowed folders.
- **Tinta Office look:** Tinta Correo's colours and control shapes (soft grey with a touch of red,
  rounder corners), light and dark; an option in Preferences › Appearance switches back to the classic look.
- From the original project (PdfCraft): many fixes to forms, signatures, printing and 3D models.

Known limits: Word, Excel and PowerPoint conversion depends on Tinta Texto, Calculo and Presenta
(previews too); OCR reads poorly under dark or coloured stamps and on guilloche backgrounds; no
signing with a DNIe or a smart card yet.
