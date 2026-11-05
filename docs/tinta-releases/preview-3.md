# Tinta PDF — Preview 3

> **Status:** 2026-11-03 · draft for the Preview 3 release (12 November 2026), to be
> finished after the 9 November upstream sync · **Revision history:** 2026-11-03 created (changes
> from Preview 2, 27 October, to #46).

Release notes for the GitHub release and the site, in Spanish, Ukrainian and English. Changes since
Preview 2 (27 October 2026). Before publishing: add what the 9 November upstream sync brings, set the
version, build the Windows installers of all five apps with `packaging/windows/tinta-build-release.ps1`
(MSI, portable zip, SHA256SUMS.txt), and run the smoke test on the installers
(`packaging/windows/tinta-smoke.ps1`, `packaging/linux/tinta-smoke.sh`).

## Español

**Tinta PDF — vista previa 3.** Versión de prueba: úsela con copias de sus documentos.

Novedades:

- **Word, Excel y PowerPoint a PDF.** Abra o arrastre un .docx, .xlsx o .pptx (y .doc, .odt, .rtf,
  .xls, .ods…): Tinta Texto, Tinta Calculo o Tinta Presenta lo convierten a PDF. También en
  Combinar archivos, Insertar páginas y el procesamiento por lotes. Necesita tener instalada la
  aplicación de Tinta Office correspondiente.
- **Reconocimiento de texto (OCR) en español, catalán, gallego, euskera, ucraniano y 8 idiomas
  más.** Lee bien fotos hechas con el móvil: páginas giradas o del revés, hojas fotografiadas
  torcidas o con sombra, dos columnas, sellos claros encima del texto. Corrige las confusiones
  típicas de i y l en español y de і, ї, є en ucraniano.
- **Firma con sello de tiempo.** Al firmar con su certificado (por ejemplo, de la ACCV) puede añadir un sello de
  tiempo de ACCV, DigiCert o FreeTSA; la ventana ya no se queda esperando al servidor. Las firmas
  de certificados emitidos en España se reconocen como válidas con los certificados raíz de Windows.
  La firma visible no muestra su NIF/DNI/NIE salvo que usted lo pida.
- **Ayuda ▸ Informar de un problema**, y el envío opcional de informes de error (solo si usted lo
  acepta al primer inicio).
- **Idiomas:** rumano, checo y lituano completos.
- **Linux:** paquete Flatpak además de .deb y AppImage.
- Del proyecto original (PdfCraft): barra de herramientas personalizable, resaltado a mano alzada,
  página en blanco con el tamaño de papel de su región, número de archivos recientes ajustable y
  muchas correcciones.

Limitaciones conocidas: la conversión de Word, Excel y PowerPoint depende de Tinta Texto, Calculo y
Presenta (todavía en vista previa); el OCR no lee bien sellos oscuros o de color sobre el texto ni
fondos de guilloche; no hay firma con DNIe ni tarjeta todavía.

## Українська

**Tinta PDF — попередня версія 3.** Тестова версія: працюйте з копіями документів.

Що нового:

- **Word, Excel і PowerPoint у PDF.** Відкрийте або перетягніть .docx, .xlsx чи .pptx (а також
  .doc, .odt, .rtf, .xls, .ods…): Tinta Texto, Tinta Calculo чи Tinta Presenta перетворять його на
  PDF. Так само в «Об'єднати файли», «Вставити сторінки» і пакетній обробці. Потрібна встановлена
  відповідна програма Tinta Office.
- **Розпізнавання тексту (OCR) українською, іспанською, каталанською, галісійською, баскською і ще
  8 мовами.** Добре читає фото з телефона: сторінки боком чи догори дриґом, аркуші, зняті під
  кутом чи в тіні, дві колонки, світлі штампи поверх тексту. Виправляє типові плутанини і/и, ї, є в
  українській та i/l в іспанській.
- **Підпис із позначкою часу.** Підписуючи своїм сертифікатом (наприклад, ACCV), можна додати
  позначку часу від ACCV, DigiCert або FreeTSA; вікно більше не чекає на сервер. Підписи
  іспанськими сертифікатами визнаються дійсними через кореневі сертифікати Windows. Видимий підпис
  не показує ваш NIF/DNI/NIE, якщо ви цього не попросите.
- **Довідка ▸ Повідомити про проблему** і необов'язкове надсилання звітів про помилки (лише якщо
  ви погодилися під час першого запуску).
- **Мови:** румунська, чеська й литовська повністю.
- **Linux:** пакет Flatpak на додачу до .deb і AppImage.
- З основного проєкту (PdfCraft): налаштовувана панель інструментів, виділення від руки, порожня
  сторінка розміру паперу вашого регіону, кількість останніх файлів у налаштуваннях і багато
  виправлень.

Відомі обмеження: перетворення Word, Excel і PowerPoint залежить від Tinta Texto, Calculo й
Presenta (теж попередні версії); OCR погано читає темні чи кольорові штампи поверх тексту та
гільйошний фон; підпису DNIe чи карткою ще немає.

## English

**Tinta PDF — Preview 3.** A test version: work on copies of your documents.

What's new:

- **Word, Excel and PowerPoint to PDF.** Open or drop a .docx, .xlsx or .pptx (and .doc, .odt,
  .rtf, .xls, .ods…): Tinta Texto, Tinta Calculo or Tinta Presenta converts it to PDF. Also in
  Combine Files, Insert Pages and batch processing. The matching Tinta Office app must be
  installed.
- **Text recognition (OCR) in Spanish, Catalan, Galician, Basque, Ukrainian and 8 more
  languages.** Reads phone photos well: pages photographed sideways or upside down, sheets shot at
  an angle or in shadow, two columns, light stamps over the text. Corrects the usual i/l mix-ups in
  Spanish and і, ї, є in Ukrainian.
- **Signing with a timestamp.** When you sign with your certificate (an ACCV one, for example) you can add a
  timestamp from ACCV, DigiCert or FreeTSA; the window no longer waits for the server. Signatures
  from Spanish certificates validate through the Windows root certificates. A visible signature
  hides your NIF/DNI/NIE unless you ask for it.
- **Help ▸ Report a problem**, and optional error reports (only if you agree at first start).
- **Languages:** Romanian, Czech and Lithuanian complete.
- **Linux:** a Flatpak bundle besides .deb and AppImage.
- From the original project (PdfCraft): a customisable toolbar, freehand highlight, blank pages in
  your region's paper size, a setting for how many recent files to keep, and many fixes.

Known limits: Word, Excel and PowerPoint conversion depends on Tinta Texto, Calculo and Presenta
(previews too); OCR reads poorly under dark or coloured stamps and on guilloche backgrounds; no
signing with a DNIe or a smart card yet.
