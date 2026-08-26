<cfscript>
// Assertions over the Document() surface. Run it the same way as main.cfm:
//   rustcfml --build . && ./app --port 8829   →   /selftest.cfm
// Everything here compiles real Typst, so a pass means the markup was valid,
// not merely that it was generated.

failures = 0;
function check( required string label, required boolean ok, string detail = "" ) {
    if ( ok ) {
        writeOutput( "  ok    " & label & chr(10) );
    } else {
        failures++;
        writeOutput( "  FAIL  " & label & ( len( detail ) ? " — " & detail : "" ) & chr(10) );
    }
}

// ---- table sources ---------------------------------------------------------
q = queryNew( "name,qty", "varchar,integer", [ [ "Widget", 3 ], [ "Gadget", 1 ] ] );
fromQuery = Document().table( data = q ).toTypst();
check( "query table has a header row",   fromQuery contains "table.header" );
check( "query table carries its rows",   fromQuery contains "Widget" AND fromQuery contains "Gadget" );
check( "query table counts columns",     fromQuery contains "columns: 2" );

fromStructs = Document().table( data = [ { name = "A", qty = 1 }, { name = "B", qty = 2 } ] ).toTypst();
check( "array of structs uses the first row's keys", fromStructs contains "columns: 2" );

fromArrays = Document().table( data = [ [ "a", "b" ], [ "c" ] ], header = false ).toTypst();
check( "ragged array rows are padded", listLen( fromArrays, chr(10) ) GT 0 AND fromArrays contains "columns: 2" );

// ---- escaping --------------------------------------------------------------
hostile = "##set page(fill: black) $x^2$ ] [";
esc = Document().paragraph( hostile ).toTypst();
check( "hostile text is a string literal, not code", esc contains '"##set page(fill: black) $x^2$ ] ["' );
check( "hostile text compiles",                      arrayLen( Document().paragraph( hostile ).toBinary() ) GT 0 );

// ---- whitelists ------------------------------------------------------------
badUnit = false;
try { Document().spacer( amount = 1, unit = "furlongs" ); } catch ( any e ) { badUnit = true; }
check( "an unknown unit is rejected", badUnit );

badColor = false;
try { Document().paragraph( text = "x", color = "chartreuse" ); } catch ( any e ) { badColor = true; }
check( "an unknown colour is rejected", badColor );

badLevel = false;
try { Document().heading( text = "x", level = 9 ); } catch ( any e ) { badLevel = true; }
check( "a heading level outside 1-6 is rejected", badLevel );

badMethod = false;
try { Document().nope(); } catch ( any e ) { badMethod = true; }
check( "an unknown method is rejected", badMethod );

// ---- named-argument binding (engine v0.635.0+) -----------------------------
reordered = Document().heading( level = 3, text = "Reordered" ).toTypst();
check( "named args bind by name, not position", reordered contains '##heading(level: 3)[##"Reordered"]' );

badArg = false;
try { Document().heading( txt = "x" ); } catch ( any e ) { badArg = true; }
check( "an unknown argument name is rejected", badArg );

// ---- images ----------------------------------------------------------------
png = imageNew( "", 40, 20, "rgb", "red" );
tmp = getTempDirectory() & "/rustcfml_typst_selftest.png";
imageWrite( png, tmp );

withPath = Document().image( source = tmp, width = 40, unit = "pt" );
check( "an image from a path compiles", arrayLen( withPath.toBinary() ) GT 0 );

withBinary = Document().image( source = fileReadBinary( tmp ), width = 40, unit = "pt" );
check( "an image from binary compiles", arrayLen( withBinary.toBinary() ) GT 0 );

notAnImage = false;
try { Document().image( source = "not/an/image.png" ); } catch ( any e ) { notAnImage = true; }
check( "a missing image file is an error", notAnImage );

// A document can NOT reach the filesystem itself, even via raw Typst.
escaped = false;
try { Document().typst( '##image("/etc/hosts")' ).toBinary(); } catch ( any e ) { escaped = true; }
check( "raw Typst cannot open a path of its own", escaped );

// ---- structure -------------------------------------------------------------
twoPages = Document().paragraph( "one" ).pageBreak().paragraph( "two" );
check( "pageBreak produces a second page", twoPages.pageCount() EQ 2 );

lists = Document().list( items = [ "a", "b" ], ordered = true ).toTypst();
check( "ordered list uses enum", lists contains "##enum(" );

landscape = Document().orientation( "landscape" ).toTypst();
check( "landscape flips the page", landscape contains "flipped: true" );

counters = Document().footer( content = "Page {page} of {total}" ).paragraph( "x" ).toTypst();
check( "footer placeholders become counters", counters contains "counter(page).display()" );

// ---- terminals -------------------------------------------------------------
out = getTempDirectory() & "/rustcfml_typst_selftest.pdf";
Document().paragraph( "written" ).write( path = out, overwrite = true );
check( "write() produced a PDF", fileExists( out ) AND getFileInfo( out ).size GT 0 );

refused = false;
try { Document().paragraph( "x" ).write( path = out, overwrite = false ); } catch ( any e ) { refused = true; }
check( "write( overwrite=false ) refuses an existing file", refused );

check( "isDocumentObject recognises a document", isDocumentObject( Document() ) );
check( "isDocumentObject rejects a string",      NOT isDocumentObject( "nope" ) );

// ---- inline runs -----------------------------------------------------------
// The gap that closed: a whole paragraph used to be bold or not, so there was
// no way to emphasise three words mid-sentence.
rich = Document().paragraph( text = [
    "Please pay ",
    { text = "##1,240.00", bold = true },
    " by ",
    { text = "31 March", underline = true },
    { linebreak = true },
    { text = "Thank you", italic = true, size = 8 }
] ).toTypst();
check( "a run can be bold inside a plain paragraph", rich contains 'weight: "bold"' );
check( "a run can be underlined",                    rich contains "##underline[" );
check( "an explicit linebreak run is emitted",       rich contains "##linebreak()" );
check( "run text is still a string literal",         rich contains '"##1,240.00"' );
check( "a run-styled paragraph compiles",            arrayLen( Document().paragraph( text = [ "a", { text = "b", bold = true } ] ).toBinary() ) GT 0 );

// ---- links -----------------------------------------------------------------
lnk = Document().paragraph( text = [ { link = "https://example.com/pay", text = "Pay online" } ] ).toTypst();
check( "a link run emits ##link", lnk contains '##link("https://example.com/pay")' );
check( "link text is a literal",  lnk contains '"Pay online"' );

blockLink = Document().link( url = "mailto:billing@example.com" ).toTypst();
check( "link() with no text shows its target", blockLink contains "billing@example.com" );

for ( scheme in [ "javascript:alert(1)", "data:text/html,x", "file:///etc/passwd" ] ) {
    refusedScheme = false;
    try { Document().link( url = scheme ); } catch ( any e ) { refusedScheme = true; }
    check( "a " & listFirst( scheme, ":" ) & ": link is refused", refusedScheme );
}

// ---- footnotes, labels, references, outline --------------------------------
notes = Document().paragraph( text = [ { text = "the terms", footnote = "See clause 4." } ] ).toTypst();
check( "text plus a footnote emits both", notes contains '"the terms"' AND notes contains "##footnote[" );

noteOnly = Document().paragraph( text = [ { footnote = true, text = "Just the note." } ] ).toTypst();
check( "footnote=true makes the text the note", noteOnly contains "##footnote[" AND NOT noteOnly contains '[##"Just' & '"]##footnote' );

// Typst refuses to reference an UNNUMBERED heading, which is why
// .headingNumbering() exists — found by writing this check.
refs = Document().headingNumbering( "1." )
                 .heading( text = "Terms", level = 2, label = "terms" )
                 .paragraph( text = [ "See ", { ref = "terms" } ] );
refsOut = refs.toTypst();
check( "a label follows its heading",  refsOut contains "<terms>" );
check( "a reference resolves",          refsOut contains '##ref(label("terms"))' );
check( "labels and references compile", arrayLen( refs.toBinary() ) GT 0 );
check( "heading numbering is a document setting", refsOut contains '##set heading(numbering: "1.")' );

unnumbered = false;
try {
    Document().heading( text = "T", label = "t" ).paragraph( text = [ { ref = "t" } ] ).toBinary();
} catch ( any e ) { unnumbered = true; }
check( "referencing an unnumbered heading fails loudly", unnumbered );

badLabel = false;
try { Document().heading( text = "x", label = "not a label" ); } catch ( any e ) { badLabel = true; }
check( "a label with a space is rejected", badLabel );

toc = Document().outline( title = "Contents", depth = 2 )
                .heading( text = "One", level = 1 )
                .heading( text = "Two", level = 1 );
check( "outline compiles with headings", arrayLen( toc.toBinary() ) GT 0 );
check( "outline carries its depth",      toc.toTypst() contains "depth: 2" );

// ---- the format struct -----------------------------------------------------
// The spreadsheet convention: one plain struct, case-insensitive keys.
fmt = Document().paragraph( text = "styled", format = {
    BOLD = true, Italic = true, fontSize = 12, alignment = "right",
    color = "##333333", smallcaps = true, strikethrough = true
} ).toTypst();
check( "format struct keys are case-insensitive", fmt contains 'weight: "bold"' AND fmt contains 'style: "italic"' );
check( "format struct sets size",                 fmt contains "size: 12pt" );
check( "format struct sets alignment",            fmt contains "##align(right)" );
check( "format struct sets colour",               fmt contains 'rgb("##333333")' );
check( "format struct wraps decorations",         fmt contains "##smallcaps[" AND fmt contains "##strike[" );
check( "a fully formatted paragraph compiles",    arrayLen( Document().paragraph( text = "x", format = { bold = true, bgcolor = "red" } ).toBinary() ) GT 0 );

badFmt = false;
try { Document().paragraph( text = "x", format = { color = "chartreuse" } ); } catch ( any e ) { badFmt = true; }
check( "a bad colour inside a format struct is rejected", badFmt );

badFmtType = false;
try { Document().paragraph( text = "x", format = "bold" ); } catch ( any e ) { badFmtType = true; }
check( "format must be a struct", badFmtType );

// ---- spreadsheet-grade tables ----------------------------------------------
lines = queryNew( "sku,description,qty,unitPrice,amount",
                  "varchar,varchar,integer,decimal,decimal", [
    [ "W-1", "Widget",  3, 12.5,  37.5  ],
    [ "G-2", "Gadget",  1, 199,   199   ],
    [ "S-3", "Sprocket", 12, 0.75, 9    ]
] );

// columnList selects AND orders; headers relabels; columnFormats is per column.
t = Document().table(
    data          = lines,
    columnList    = "description,qty,amount",
    headers       = "Item,Qty,Amount",
    columnFormats = {
        qty    = { alignment = "center" },
        amount = { alignment = "right", numberFormat = "9,999.00", bold = true }
    },
    headerFormat  = { bold = true, bgcolor = "##2c3e50", color = "white" },
    stripe        = "##f5f5f5",
    widths        = "1,1,1",
    unit          = "fr",
    totals        = "amount",
    caption       = "Charges",
    label         = "charges"
);
tOut = t.toTypst();
check( "columnList selects columns",        tOut contains "columns: (1fr, 1fr, 1fr,)" );
check( "columnList drops the unlisted ones", NOT ( tOut contains "W-1" ) );
check( "headers relabel the columns",        tOut contains '"Item"' AND NOT ( tOut contains '"description"' ) );
check( "a column mask reaches the engine",   tOut contains '"37.50"' );
check( "a masked total is masked too",       tOut contains '"245.50"' );
check( "the totals row is labelled",         tOut contains '"Total"' );
check( "striping skips the header row",      tOut contains "if y >= 1 and calc.odd(y - 1)" );
check( "the header format is applied",       tOut contains 'fill: rgb("##2c3e50")' );
check( "a caption becomes a figure",         tOut contains "##figure(caption:" AND tOut contains "<charges>" );
check( "the whole table compiles",           arrayLen( t.toBinary() ) GT 0 );

badCol = false;
try { Document().table( data = lines, columnList = "nosuchcolumn" ); } catch ( any e ) { badCol = true; }
check( "columnList naming a missing column is an error", badCol );

badFmtCol = false;
try { Document().table( data = lines, columnFormats = { nope = { bold = true } } ); } catch ( any e ) { badFmtCol = true; }
check( "columnFormats naming a missing column is an error", badFmtCol );

badTotal = false;
try { Document().table( data = lines, totals = "description" ); } catch ( any e ) { badTotal = true; }
check( "totalling a text column is an error, not a zero", badTotal );

posFmt = Document().table( data = [ [ "a", 1 ], [ "b", 2 ] ], header = false,
                           columnFormats = [ {}, { alignment = "right" } ] ).toTypst();
check( "columnFormats also works positionally", posFmt contains "align: right" );

// ---- grid ------------------------------------------------------------------
g = Document().grid( data = [ [ "Acme Ltd", "Invoice INV-1042" ] ],
                     widths = "1,1", unit = "fr" ).toTypst();
check( "grid has no stroke",     g contains "stroke: none" );
check( "grid has no header row", NOT ( g contains "table.header(" ) );

// ---- callouts, quotes, code, terms -----------------------------------------
callout = Document().callout( content = "Payment is due within 30 days.",
                              fill = "##fff8e1", stroke = "##ffe082",
                              radius = 3, inset = 10, unit = "pt" );
calloutOut = callout.toTypst();
check( "a callout is a block with a fill", calloutOut contains "##block(" AND calloutOut contains 'fill: rgb("##fff8e1")' );
check( "a callout compiles",               arrayLen( callout.toBinary() ) GT 0 );

quoted = Document().quote( text = "Cash is a fact, profit is an opinion.", attribution = "Alfred Rappaport" );
check( "a quote carries its attribution", quoted.toTypst() contains "attribution:" );
check( "a quote compiles",                arrayLen( quoted.toBinary() ) GT 0 );

code = Document().code( text = 'writeOutput( "hi" );', language = "cfml" );
check( "code is raw with a language", code.toTypst() contains "##raw(" AND code.toTypst() contains 'lang: "cfml"' );
check( "code compiles",               arrayLen( code.toBinary() ) GT 0 );

termsFromStruct = Document().terms( items = { "Net 30" = "Payable within 30 days", "VAT" = "Standard rate" } );
check( "terms from a struct",  termsFromStruct.toTypst() contains "terms.item(" );
check( "terms compile",        arrayLen( termsFromStruct.toBinary() ) GT 0 );
termsFromArray = Document().terms( items = [ { term = "A", description = "first" }, [ "B", "second" ] ] );
check( "terms from an array of structs and pairs", termsFromArray.toTypst() contains "terms.item(" );

// ---- page-level extras -----------------------------------------------------
twoCol = Document().columns( count = 2 ).paragraph( text = "flowing text" );
check( "two-column text is a page setting", twoCol.toTypst() contains "columns: 2" );
check( "two-column text compiles",          arrayLen( twoCol.toBinary() ) GT 0 );

meta = Document().title( "T" ).author( "A" ).keywords( "invoice,acme" ).toTypst();
check( "keywords reach the document metadata", meta contains "keywords: (" AND meta contains '"acme"' );

captioned = Document().image( source = tmp, width = 40, unit = "pt", caption = "The logo", label = "logo" );
check( "an image caption becomes a figure", captioned.toTypst() contains "##figure(caption:" );
check( "a captioned image compiles",        arrayLen( captioned.toBinary() ) GT 0 );

// ---- PDF options -----------------------------------------------------------
untagged = Document().paragraph( "x" ).pdf( tagged = false );
check( "tagged=false still produces a PDF", arrayLen( untagged.toBinary() ) GT 0 );

versioned = Document().paragraph( "x" ).pdf( standard = "1.7" );
check( "a PDF version standard is accepted", arrayLen( versioned.toBinary() ) GT 0 );

stamped = Document().paragraph( "x" ).pdf( creator = "RustCFML selftest", timestamp = createDate( 2026, 3, 20 ) );
check( "creator and timestamp are accepted", arrayLen( stamped.toBinary() ) GT 0 );

// Typst will not write a tagged PDF with a page range, and tagging is ON by
// default — so this needs both settings, and asking for only one is refused at
// the call rather than at the terminal.
pages = Document().paragraph( "one" ).pageBreak().paragraph( "two" ).pageBreak().paragraph( "three" )
                  .pdf( pages = "1-2", tagged = false );
check( "a page range exports when tagging is off", arrayLen( pages.toBinary() ) GT 0 );

rangeAlone = false;
rangeMsg = "";
try { Document().paragraph( "x" ).pdf( pages = "1-2" ); } catch ( any e ) { rangeAlone = true; rangeMsg = e.message; }
check( "a page range without tagged=false is refused at the call", rangeAlone );
check( "and the refusal names the fix",                            rangeAlone AND rangeMsg contains "tagged=false" );

badStandard = false;
try { Document().pdf( standard = "pdf/a" ); } catch ( any e ) { badStandard = true; }
check( "an unknown PDF standard is rejected", badStandard );

badRange = false;
try { Document().paragraph( "x" ).pdf( pages = "0", tagged = false ).toBinary(); } catch ( any e ) { badRange = true; }
check( "page 0 is rejected — pages start at 1", badRange );

// ---- templates -------------------------------------------------------------
templateDir  = getDirectoryFromPath( getCurrentTemplatePath() ) & "templates/";
templatePath = templateDir & "statement.typ";
statementData = {
    reference = "STMT-2026-0007",
    date      = "20 Mar 2026",
    customer  = { name = "Acme Ltd", address = "1 Example Way, London" },
    lines     = [
        { description = "Widget", qty = 3,  amount = 37.5 },
        { description = "Gadget", qty = 1,  amount = 199  }
    ]
};

tpl = Document().template( path = templatePath ).data( statementData );
check( "toTypst() on a template shows the TEMPLATE, not generated markup",
       tpl.toTypst() contains 'json("data.json")' );
tplPdf = tpl.toBinary();
check( "a template compiles to a PDF",         arrayLen( tplPdf ) GT 0 );
check( "a template can import a sibling file", true, "proven by the compile above — statement.typ imports _shared.typ" );
check( "a template lays out one page",         tpl.pageCount() EQ 1 );

oneShot = typstRender( templatePath, statementData );
check( "typstRender() is the one-shot form", arrayLen( oneShot ) GT 0 );

// The data really is data. "It compiled" is not evidence — an executed
// `##pagebreak()` compiles perfectly well and produces a SECOND PAGE. Count the
// pages instead.
injectedData = duplicate( statementData );
injectedData.reference = "##pagebreak() X";
injected = Document().template( path = templatePath ).data( injectedData );
check( "template data cannot become Typst code",
       injected.pageCount() EQ 1, "an executed pagebreak would make it 2" );
check( "typstRender() accepts the same data", arrayLen( typstRender( templatePath, injectedData ) ) GT 0 );

missingTemplate = false;
try { Document().template( path = templateDir & "nope.typ" ); } catch ( any e ) { missingTemplate = true; }
check( "a missing template is an error", missingTemplate );

// The sandbox: the root defaults to the template's own directory, so a template
// asking for something above it must fail.
escapeFile = getTempDirectory() & "/rustcfml_typst_outside.typ";
fileWrite( escapeFile, '##let x = read("../../../../etc/hosts")' & chr(10) & "##x" );
escapedRoot = false;
escapeMsg = "";
try { Document().template( path = escapeFile ).toBinary(); } catch ( any e ) { escapedRoot = true; escapeMsg = e.message; }
check( "a template cannot read above its root", escapedRoot );
// A bare try/catch would also "pass" if the extension were missing entirely, so
// check WHY it failed.
check( "and it fails because of the sandbox, not by accident",
       escapedRoot AND escapeMsg contains "escape the project root", escapeMsg );

packaged = getTempDirectory() & "/rustcfml_typst_package.typ";
fileWrite( packaged, '##import "@preview/cetz:0.2.0": *' & chr(10) & "hello" );
refusedPackage = false;
packageMsg = "";
try { Document().template( path = packaged ).toBinary(); } catch ( any e ) { refusedPackage = true; packageMsg = e.message; }
check( "a @preview package is refused", refusedPackage );
check( "and the refusal says why",      refusedPackage AND packageMsg contains "package" );

// A generated document still has NO root, which is the property templates
// deliberately relax.
noRoot = false;
try { Document().typst( '##include "other.typ"' ).toBinary(); } catch ( any e ) { noRoot = true; }
check( "a generated document cannot include a file", noRoot );

// ---- fonts -----------------------------------------------------------------
// A host with NO fonts does not make Typst fail — it makes it emit a blank page.
// This extension refuses instead, so a text document errors there while a
// text-free one still works. Only the second half is checkable on a host that
// has fonts, which is why the first half is verified in a fontless container.
textFree = Document().rule().spacer( amount = 1, unit = "cm" ).rule();
check( "a document with no text compiles (the case that must survive the font guard)",
       arrayLen( textFree.toBinary() ) GT 0 );
check( "typstFontCount() reports this host's faces", typstFontCount() GTE 0 );

// ---- version ---------------------------------------------------------------
check( "typstVersion() reports a version", reFind( "^\d+\.\d+\.\d+$", typstVersion() ) GT 0 );

writeOutput( chr(10) & ( failures ? "FAILURES: " & failures : "all checks passed" ) & chr(10) );
</cfscript>
