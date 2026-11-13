//! The table: every user-facing string, one row per message.
//!
//! A row is `Variant = [English, Russian]`. The variant is the key, the pair is the translation, and
//! the array length is [`Lang::COUNT`], so adding a language breaks every row until it is filled in.
//! Rows are grouped by the surface that draws them; the group comment is a reading aid, not data.

use crate::Lang;

messages! {
    // ── The table's own tests ───────────────────────────────────────────────
    TestPair = ["{} and {}", "{} и {}"],
    TestPointOne = ["{} point", "{} точка"],
    TestPointFew = ["{} points", "{} точки"],
    TestPointMany = ["{} points", "{} точек"],









    // ── Header and file menu ────────────────────────────────────────────────
    HeaderHome = ["Home", "Главная"],
    HeaderSave = ["Save  (Ctrl+S)", "Сохранить  (Ctrl+S)"],
    HeaderUndo = ["Undo  (Ctrl+Z)", "Отменить  (Ctrl+Z)"],
    HeaderNothingToUndo = ["Nothing to undo", "Нечего отменять"],
    HeaderRedo = ["Redo  (Ctrl+Y)", "Повторить  (Ctrl+Y)"],
    HeaderNothingToRedo = ["Nothing to redo", "Нечего повторять"],
    HeaderNewScene = ["New scene  (Ctrl+N)", "Новая сцена  (Ctrl+N)"],
    HeaderUnsaved = ["Unsaved changes", "Есть несохранённые изменения"],
    HeaderCloseTab = ["Close the tab", "Закрыть вкладку"],
    CollapseAllPanels = ["Collapse all panels  (Ctrl+Shift+0)", "Свернуть все панели  (Ctrl+Shift+0)"],
    ExpandAllPanels = ["Expand all panels  (Ctrl+Shift+0)", "Развернуть все панели  (Ctrl+Shift+0)"],
    HideAllPanels = ["Hide all panels  (Ctrl+0)", "Скрыть все панели  (Ctrl+0)"],
    ShowAllPanels = ["Show all panels  (Ctrl+0)", "Показать все панели  (Ctrl+0)"],
    FoldRibbon = ["Fold the ribbon", "Свернуть ленту"],
    UnfoldRibbon = ["Unfold the ribbon", "Развернуть ленту"],
    ExpandPanel = ["Expand panel", "Развернуть панель"],
    File = ["File", "Файл"],
    MenuNew = ["New", "Новый"],
    MenuOpen = ["Open…", "Открыть…"],
    MenuSave = ["Save", "Сохранить"],
    MenuSaveAs = ["Save As…", "Сохранить как…"],
    MenuCloseTab = ["Close tab", "Закрыть вкладку"],
    MenuExit = ["Exit", "Выход"],
    Untitled = ["Untitled", "Без имени"],
    CloseTitle = ["Close “{}”?", "Закрыть «{}»?"],
    CloseBody = [
        "This scene has unsaved changes. Closing the tab discards them.",
        "В сцене есть несохранённые изменения. Закрытие вкладки отбросит их."
    ],
    DiscardAndClose = ["Discard and close", "Отбросить и закрыть"],
    QuitOneBody = [
        "One open scene has unsaved changes. Quitting discards them.",
        "В одной открытой сцене есть несохранённые изменения. Выход отбросит их."
    ],
    QuitManyOne = ["{} open scene has unsaved changes. Quitting discards them.", "{} открытая сцена содержит несохранённые изменения. Выход отбросит их."],
    QuitManyFew = ["{} open scenes have unsaved changes. Quitting discards them.", "{} открытые сцены содержат несохранённые изменения. Выход отбросит их."],
    QuitManyMany = ["{} open scenes have unsaved changes. Quitting discards them.", "{} открытых сцен содержат несохранённые изменения. Выход отбросит их."],
    QuitTitle = ["Quit {}?", "Выйти из {}?"],
    DiscardAndQuit = ["Discard and quit", "Отбросить и выйти"],
    OpenSceneTitle = ["Open scene", "Открыть сцену"],
    OpenButton = ["Open", "Открыть"],
    SaveSceneAsTitle = ["Save scene as", "Сохранить сцену как"],
    SaveButton = ["Save", "Сохранить"],
    OpenNewTabHint = [
        "The scene opens in a new tab; the open scenes are left as they are.",
        "Сцена откроется в новой вкладке; открытые сцены останутся как есть."
    ],
    PathLabel = ["Path", "Путь"],
    NearbyScenes = ["Nearby scenes", "Сцены рядом"],
    EnterFilePath = ["Enter a file path.", "Введите путь к файлу."],
    NoticeNoSettingsLocation = [
        "No workspace settings location; using defaults.",
        "Нет расположения настроек рабочей области; используются значения по умолчанию."
    ],
    NoticeNoSavedSettings = [
        "No saved workspace settings yet; using the defaults.",
        "Сохранённых настроек ещё нет; используются значения по умолчанию."
    ],
    NoticeReadFailed = [
        "Could not read the workspace settings ({}); using defaults.",
        "Не удалось прочитать настройки рабочей области ({}); используются значения по умолчанию."
    ],
    NoticeUnusable = [
        "Could not use the workspace settings ({}); using defaults.",
        "Не удалось применить настройки рабочей области ({}); используются значения по умолчанию."
    ],
    NoticeUnknownPalette = [
        "The saved palette “{}” is unknown to this build; {} is in use.",
        "Сохранённая палитра «{}» неизвестна этой сборке; используется {}."
    ],

    // ── Status bar ──────────────────────────────────────────────────────────
    LockedStatus = ["Locked · {}", "Заблокировано · {}"],
    StatusSelectionCount = ["Selected: {}", "Выбрано: {}"],
    StatusSize = ["{} × {} × {}", "{} × {} × {}"],
    StatusBanding = ["edge {}", "кромка {}"],
    StatusReady = ["Ready", "Готово"],
    StatusRebuilding = ["Rebuilding…", "Перестроение…"],
    StatusScale = ["{} %", "{} %"],
    UnlockHint = [
        "Unlock it in the scene tree to move or delete it",
        "Снимите блокировку в дереве сцены, чтобы переместить или удалить"
    ],
    AddingTo = ["Adding to: {}", "Добавление в: {}"],
    CreateAtRoot = ["Create at the scene root again", "Снова создавать в корне сцены"],
    ContourEditStatusOne = ["Contour edit · {} segment", "Редактирование контура · {} сегмент"],
    ContourEditStatusFew = ["Contour edit · {} segments", "Редактирование контура · {} сегмента"],
    ContourEditStatusMany = ["Contour edit · {} segments", "Редактирование контура · {} сегментов"],
    ContourModifiedSuffix = [" · modified", " · изменён"],
    AlongContour = ["Along contour", "Вдоль контура"],
    FreeMove = ["Free move", "Свободное перемещение"],
    ContourAlongHint = [
        "Drag corners along their segment  ·  double-click a side to split  ·  Enter apply  ·  Esc cancel",
        "Перетаскивайте углы вдоль их сегмента  ·  двойной щелчок по стороне делит её  ·  Enter применить  ·  Esc отмена"
    ],
    ContourFreeHint = [
        "Drag corners freely  ·  double-click a side to split  ·  Enter apply  ·  Esc cancel",
        "Перетаскивайте углы свободно  ·  двойной щелчок по стороне делит её  ·  Enter применить  ·  Esc отмена"
    ],
    DrawStatus = ["Draw · {}{}{}", "Рисование · {}{}{}"],
    PointsSuffixOne = [" · {} point", " · {} точка"],
    PointsSuffixFew = [" · {} points", " · {} точки"],
    PointsSuffixMany = [" · {} points", " · {} точек"],
    DrawArcSuffix = [" · arc", " · дуга"],
    DrawLineSuffix = [" · line", " · линия"],
    SnapTenMillimetres = ["Snap 10 mm  (G)", "Привязка 10 мм  (G)"],
    FinishEnter = ["Finish  (Enter)", "Завершить  (Enter)"],
    ApplyEnter = ["Apply  (Enter)", "Применить  (Enter)"],
    CancelEsc = ["Cancel  (Esc)", "Отмена  (Esc)"],
    DrawHint = [
        "LMB add point  ·  Tab line/arc  ·  Shift axis lock  ·  Alt ignores snaps  ·  Enter/RMB finish  ·  Esc cancel",
        "ЛКМ добавить точку  ·  Tab линия/дуга  ·  Shift фиксация оси  ·  Alt игнорирует привязки  ·  Enter/ПКМ завершить  ·  Esc отмена"
    ],
    PrimitiveStatus = ["Primitive · {} {}", "Примитив · {} {}"],
    PrimitivePlaceHint = ["LMB place on the work plane  ·  Esc cancel", "ЛКМ разместить на рабочей плоскости  ·  Esc отмена"],
    NavigationStatus = ["Navigation · {}", "Навигация · {}"],
    GestureOrbit = ["Drag in the viewport to orbit  ·  Esc done", "Перетаскивайте в области просмотра для облёта  ·  Esc готово"],
    GesturePan = ["Drag in the viewport to pan  ·  Esc done", "Перетаскивайте в области просмотра для панорамы  ·  Esc готово"],
    GestureZoom = ["Drag vertically to zoom  ·  Esc done", "Перетаскивайте вертикально для масштаба  ·  Esc готово"],
    NavOrbitLabel = ["Orbit  ·  drag in the viewport", "Облёт  ·  перетаскивание в области просмотра"],
    NavPanLabel = ["Pan  ·  drag in the viewport", "Панорама  ·  перетаскивание в области просмотра"],
    NavZoomLabel = ["Zoom  ·  drag vertically", "Масштаб  ·  перетаскивание по вертикали"],
    DoneEsc = ["Done  (Esc)", "Готово  (Esc)"],
    Gizmo = ["Gizmo", "Гизмо"],
    RollToDegree = ["Roll to {}°", "Крен до {}°"],
    HomeHint = ["Home  ·  back to the axonometric view", "Домой  ·  вернуться к аксонометрии"],
    ProjectionOrthoShort = ["Ortho", "Орто"],
    ProjectionPerspShort = ["Persp", "Персп"],
    OrientationReadout = ["Pitch {} · Yaw {} · Roll {} · {}", "Тангаж {} · Рыскание {} · Крен {} · {}"],
    Move = ["Move", "Перемещение"],
    Rotate = ["Rotate", "Вращение"],
    MeshEditStatusOne = ["Mesh edit · {} vertex selected", "Редактирование сетки · выбрана {} вершина"],
    MeshEditStatusFew = ["Mesh edit · {} vertices selected", "Редактирование сетки · выбрано {} вершины"],
    MeshEditStatusMany = ["Mesh edit · {} vertices selected", "Редактирование сетки · выбрано {} вершин"],
    MeshFaceStatusOne = ["Mesh edit · {} face selected", "Редактирование сетки · выбрана {} грань"],
    MeshFaceStatusFew = ["Mesh edit · {} faces selected", "Редактирование сетки · выбрано {} грани"],
    MeshFaceStatusMany = ["Mesh edit · {} faces selected", "Редактирование сетки · выбрано {} граней"],
    MeshEditHint = [
        "LMB select  ·  Shift/Ctrl add  ·  Esc done  ·  MMB orbit",
        "ЛКМ выбрать  ·  Shift/Ctrl добавить  ·  Esc готово  ·  СКМ облёт"
    ],
    ViewportHint = [
        "LMB select  ·  double-click enter  ·  Esc leave  ·  MMB orbit  ·  Shift+MMB pan  ·  Ctrl+MMB zoom",
        "ЛКМ выбрать  ·  двойной щелчок войти  ·  Esc выйти  ·  СКМ облёт  ·  Shift+СКМ панорама  ·  Ctrl+СКМ масштаб"
    ],

    // ── Side panels and viewport cluster ────────────────────────────────────
    PanelScene = ["Scene", "Сцена"],
    PanelInspector = ["Inspector", "Инспектор"],
    FoldPanel = ["Fold panel", "Свернуть панель"],
    OpenPanel = ["Open the {} panel", "Открыть панель «{}»"],
    Layout = ["Layout", "Раскладка"],
    LayoutOneView = ["One view", "Один вид"],
    LayoutTwoVertical = ["Two vertical", "Два по вертикали"],
    LayoutTwoHorizontal = ["Two horizontal", "Два по горизонтали"],
    LayoutFourViews = ["Four views", "Четыре вида"],
    ViewsOfMax = ["{} of {} views · drag a border to resize", "{} из {} видов · перетащите границу, чтобы изменить размер"],
    Restore = ["Restore", "Восстановить"],
    Expand = ["Expand", "Развернуть"],
    DisplaySettings = ["Display settings", "Настройки отображения"],
    ViewPreset = ["View preset", "Предустановка вида"],
    DisplayHeading = ["Display", "Отображение"],
    ShowProjectionAngles = ["Show projection and angles", "Показывать проекцию и углы"],
    ProjectionAnglesOn = [
        "The orientation readout is drawn under the cube",
        "Отсчёт ориентации рисуется под кубом"
    ],
    ProjectionAnglesOff = [
        "Draw the pitch, yaw, roll and the projection under the cube",
        "Рисовать тангаж, рыскание, крен и проекцию под кубом"
    ],
    ShowOrientationWidget = ["Show the orientation cube", "Показывать куб ориентации"],
    OrientationWidgetOn = [
        "The corner cube, its home button and the projection toggle are drawn beside the panels",
        "Куб в углу, кнопка «домой» и переключатель проекции рисуются рядом с панелями"
    ],
    OrientationWidgetOff = [
        "Hide the corner cube, its home button and the projection toggle; the views stay reachable from the cluster",
        "Скрыть куб в углу, кнопку «домой» и переключатель проекции; виды остаются доступны из кластера"
    ],
    AuthoredCameraOwns = ["An authored camera owns the projection", "Проекцию задаёт авторская камера"],
    ContactShadows = ["Contact shadows", "Контактные тени"],
    ContactShadowsHint = [
        "A soft shadow under every object that rests on the floor.",
        "Мягкая тень под каждым объектом, стоящим на полу."
    ],
    GridStrength = ["Floor grid", "Сетка пола"],
    EdgesStrength = ["Outlines", "Контуры"],
    ShadowStrength = ["Contact shadow", "Контактная тень"],
    GridStrengthHint = [
        "Brightness of the work-plane floor grid; 0 hides it.",
        "Яркость сетки пола; 0 скрывает её."
    ],
    EdgesStrengthHint = [
        "Strength of the form-outline pass; 0 leaves the fill only.",
        "Сила контурной обводки; 0 оставляет только заливку."
    ],
    ShadowStrengthHint = [
        "Strength of the soft shadow under the objects that rest on the floor.",
        "Сила мягкой тени под объектами, стоящими на полу."
    ],
    DisplayConfigure = ["Configure…", "Настроить…"],
    DisplayConfigureHint = [
        "Opens Settings › View for the display strengths.",
        "Открывает «Настройки › Вид» для силы отображения."
    ],
    ResetDisplay = ["Reset display", "Сбросить отображение"],
    ResetDisplayHint = [
        "Restores the floor grid, outline and contact shadow strengths to 60 / 70 / 40.",
        "Возвращает силу сетки пола, контуров и контактной тени: 60 / 70 / 40."
    ],
    SectionAdditional = ["Additional", "Дополнительно"],
    WorkPlane = ["Work plane · {}", "Рабочая плоскость · {}"],
    PerspectiveViewHint = [
        "Perspective view; switch to orthographic  (O)",
        "Перспективный вид; переключить на ортогональный  (O)"
    ],
    OrthographicViewHint = [
        "Orthographic view; switch to perspective  (O)",
        "Ортогональный вид; переключить на перспективный  (O)"
    ],

    // ── Machining ───────────────────────────────────────────────────────────
    NoMachining = ["No machining.", "Обработок нет."],
    MachiningClassNotAdmitted = [
        "The class does not allow this machining.",
        "Класс не допускает эту обработку."
    ],
    MachiningCutout = ["Cutout", "Вырез"],
    MachiningCategoryCut = ["Cut", "Вырезание"],
    ShowHideOperation = ["Show or hide this operation", "Показать или скрыть операцию"],
    MoveEarlier = ["Move earlier", "Переместить раньше"],
    MoveLater = ["Move later", "Переместить позже"],
    RemoveOperation = ["Remove operation", "Удалить операцию"],
    AddOperation = ["Add operation", "Добавить операцию"],
    FaceWith = ["Face {}", "Грань {}"],
    EdgeWith = ["Edge {}", "Кромка {}"],
    AnchorWith = ["Anchor  {}", "Привязка: {}"],
    Entry = ["Entry", "Вход"],
    Through = ["Through", "Насквозь"],
    Blind = ["Blind", "Глухой"],
    CutoutReshapeHint = [
        "Reshapes this ring in the viewport. Undo restores the stored ring.",
        "Изменяет это кольцо в области просмотра. Отмена вернёт сохранённое кольцо."
    ],
    ArcSuffixOne = [" · {} arc", " · {} дуга"],
    ArcSuffixFew = [" · {} arcs", " · {} дуги"],
    ArcSuffixMany = [" · {} arcs", " · {} дуг"],
    Depth = ["Depth", "Глубина"],
    NotALength = ["Not a length: {}", "Не длина: {}"],
    NotAnAngle = ["Not an angle: {}", "Не угол: {}"],

    // ── Object card ─────────────────────────────────────────────────────────
    ObjectNameEmpty = ["Object name must not be empty.", "Имя объекта не должно быть пустым."],
    SectionGeometry = ["Geometry", "Геометрия"],
    VerticesCount = ["Vertices  {}", "Вершин: {}"],
    FacesCount = ["Faces  {}", "Граней: {}"],
    PrimitiveSourceTruth = [
        "The parameters are the source of truth; the mesh is derived.",
        "Параметры — источник истины; сетка выводится из них."
    ],
    ContourSegmentsCount = ["Contour segments  {}", "Сегментов контура: {}"],
    HeightRow = ["Height  {}", "Высота: {}"],
    ContourSourceTruth = [
        "The contour and height are the source of truth; the mesh is derived.",
        "Контур и высота — источник истины; сетка выводится из них."
    ],
    PathSegmentsCount = ["Path segments  {}", "Сегментов пути: {}"],
    ClosedChain = ["Closed chain", "Замкнутая цепь"],
    OpenChain = ["Open chain", "Разомкнутая цепь"],
    PathSourceTruth = ["The drawing is the source of truth; it has no mesh yet.", "Чертёж — источник истины; сетки пока нет."],
    MeshSourceTruth = ["Editable as a mesh; undo is its only history.", "Редактируется как сетка; история — только отмена."],
    SectionFigure = ["Figure", "Фигура"],
    FieldRadius = ["Radius", "Радиус"],
    FieldWidth = ["Width", "Ширина"],
    FieldEndAngle = ["End angle", "Конечный угол"],
    FigureEditingInViewport = [
        "Editing the figure in the viewport; finish or cancel it first.",
        "Фигура редактируется в области просмотра; сначала завершите или отмените."
    ],
    SectionParameters = ["Parameters", "Параметры"],
    PrimitiveRefused = ["The primitive was refused: {}", "Примитив отклонён: {}"],
    FigureRefused = ["The figure was refused: {}", "Фигура отклонена: {}"],
    FieldEdge = ["Edge", "Ребро"],
    FieldBands = ["Bands", "Полосы"],
    SectionMeshEditing = ["Mesh editing", "Редактирование сетки"],
    CollapseBeforeEditing = [
        "Collapse the object to a mesh before editing its elements.",
        "Сверните объект в сетку, прежде чем редактировать элементы."
    ],
    PathHasNoMesh = ["A path object has no mesh; its drawing is the source of truth.", "У объекта-пути нет сетки; источник истины — чертёж."],
    EditMesh = ["Edit mesh", "Редактировать сетку"],
    VerticesPlural = ["Vertices", "Вершины"],
    FacesPlural = ["Faces", "Грани"],
    SelectSingleObjectMesh = ["Select only this object to edit its mesh.", "Выберите только этот объект, чтобы редактировать его сетку."],
    SelectedVerticesCount = ["Selected vertices  {}", "Выбрано вершин: {}"],
    MoreCountOne = ["… and {} more", "… и ещё {}"],
    MoreCountFew = ["… and {} more", "… и ещё {}"],
    MoreCountMany = ["… and {} more", "… и ещё {}"],
    ClickVertexHint = [
        "Click a vertex in the viewport; Shift or Ctrl adds it to the selection.",
        "Щёлкните вершину в области просмотра; Shift или Ctrl добавит её к выделению."
    ],
    SelectedFacesCount = ["Selected faces  {}", "Выбрано граней: {}"],
    ClickFaceHint = [
        "Click a face in the viewport; Shift or Ctrl adds it to the selection.",
        "Щёлкните грань в области просмотра; Shift или Ctrl добавит её к выделению."
    ],
    SectionContourEditing = ["Contour editing", "Редактирование контура"],
    ObjectContourEditingInViewport = [
        "Editing the contour in the viewport. Apply or cancel in the viewport toolbar.",
        "Контур редактируется в области просмотра. Примените или отмените на панели инструментов области просмотра."
    ],
    SelectSingleObjectContour = ["Select only this object to edit its contour.", "Выберите только этот объект, чтобы редактировать его контур."],
    DragCornersHint = ["Drag corners in the viewport; double-click a side to split it.", "Перетаскивайте углы в области просмотра; двойной щелчок по стороне делит её."],
    SectionContour = ["Contour", "Контур"],
    ContourHasArcs = [
        "This contour has arcs; corners cannot describe it, so the text editor is off.",
        "В этом контуре есть дуги; углы не описывают его, поэтому текстовый редактор отключён."
    ],
    ContourTextHint = ["{} points; one per line as `X, Y` in millimetres.", "{} точек; по одной в строке в виде `X, Y` в миллиметрах."],
    ApplyContour = ["Apply contour", "Применить контур"],
    SectionCollapse = ["Collapse", "Свернуть"],
    SelectSingleObjectCollapse = ["Select only this object to collapse it.", "Выберите только этот объект, чтобы свернуть его."],
    CollapseWarning = [
        "Baking the parameters into a mesh deletes them as parameters. Undo is the only way back.",
        "Превращение параметров в сетку удаляет их как параметры. Вернуть можно только отменой."
    ],
    CollapseToMesh = ["Collapse to mesh", "Свернуть в сетку"],
    CollapseToMeshEllipsis = ["Collapse to mesh…", "Свернуть в сетку…"],
    LineExpectedPoint = ["Line {}: expected `X, Y` in millimetres.", "Строка {}: ожидается `X, Y` в миллиметрах."],
    LineWithError = ["Line {}: {}.", "Строка {}: {}."],
    PrimitiveBadgeMesh = ["Mesh", "Сетка"],
    PrimitiveBadgePrimitive = ["Primitive", "Примитив"],
    PrimitiveBadgeConstructive = ["Constructive", "Конструктив"],
    PrimitiveBadgePath = ["Path", "Путь"],

    // ── Panel card ──────────────────────────────────────────────────────────
    PanelEditingInViewport = [
        "Editing in the viewport. Apply or cancel in the viewport toolbar.",
        "Редактирование в области просмотра. Примените или отмените на панели инструментов области просмотра."
    ],
    PanelFinishContour = [
        "Finish the current contour edit in the viewport first.",
        "Сначала завершите текущее редактирование контура в области просмотра."
    ],
    PanelSelectSingle = [
        "Select only this panel to edit its contour.",
        "Выберите только эту панель, чтобы редактировать её контур."
    ],
    PanelEditContour = ["Edit contour", "Редактировать контур"],
    CornerNone = ["None", "Нет"],
    CornerRadius = ["Radius", "Радиус"],
    CornerChamfer = ["Chamfer", "Фаска"],
    PanelNameEmpty = ["Panel name must not be empty.", "Имя панели не должно быть пустым."],
    SectionDimensions = ["Dimensions", "Размеры"],
    SectionNominalSize = ["Nominal size", "Номинальный размер"],
    PanelContourSetsSize = [
        "The contour ring sets the length and width.",
        "Длину и ширину задаёт контурное кольцо."
    ],
    PanelClassLengthOnly = [
        "The class fixes the width: this object resizes by length only.",
        "Класс фиксирует ширину: этот объект меняется только по длине."
    ],
    PanelClassSizeFixed = [
        "The class fixes the size: this object is not resized.",
        "Класс фиксирует размер: этот объект не изменяется."
    ],
    Length = ["Length", "Длина"],
    Width = ["Width", "Ширина"],
    LengthField = ["Length: {}", "Длина: {}"],
    WidthField = ["Width: {}", "Ширина: {}"],
    SectionResolved = ["Resolved", "Расчётные"],
    SectionMaterial = ["Material", "Материал"],
    SectionShape = ["Shape", "Форма"],
    SectionBanding = ["Banding", "Кромка"],
    PanelClassNoBanding = ["The class does not allow edge banding.", "Класс не допускает кромление."],
    PanelClassNoContour = ["The class does not allow outline edits.", "Класс не допускает редактирование контура."],
    SectionMachining = ["Machining", "Обработка"],
    SectionDiagnostics = ["Diagnostics", "Диагностика"],
    PanelMaterialMissingCatalogue = [
        "Panel material {} is missing from the catalogue.",
        "Материал панели {} отсутствует в каталоге."
    ],
    MissingMaterial = ["Missing material {}", "Материал {} отсутствует"],
    NoMaterial = ["No material", "Нет материала"],
    SectionOutline = ["Outline", "Контур"],
    PanelContourRingOne = ["Contour ring · {} segment", "Контурное кольцо · {} сегмент"],
    PanelContourRingFew = ["Contour ring · {} segments", "Контурное кольцо · {} сегмента"],
    PanelContourRingMany = ["Contour ring · {} segments", "Контурное кольцо · {} сегментов"],
    ArcsCount = ["Arcs  {}", "Дуги: {}"],
    BakeOutlineHint = [
        "Bakes the outline into a ring. Corners, chamfers and bows become segments; undo restores them.",
        "Преобразует контур в кольцо. Углы, фаски и прогибы становятся сегментами; отмена вернёт их."
    ],
    ContourReshapeHint = [
        "Reshapes the ring in the viewport. Undo restores it.",
        "Изменяет кольцо в области просмотра. Отмена вернёт прежнее."
    ],
    SectionBowedEdges = ["Bowed edges", "Изогнутые кромки"],
    CornerWith = ["Corner {}", "Угол {}"],
    Inward = ["in", "внутрь"],
    Outward = ["out", "наружу"],
    Clear = ["Clear", "Очистить"],
    NoBanding = ["No banding.", "Кромок нет."],
    AddChamferOrRadius = [
        "Add a chamfer or radius to band a segment.",
        "Добавьте фаску или радиус, чтобы оклеить сегмент."
    ],
    AllSegmentsBanded = ["All existing segments are banded.", "Все существующие сегменты оклеены."],
    AddEdgeMaterial = [
        "Add an edge material to the catalogue first.",
        "Сначала добавьте материал кромки в каталог."
    ],
    AddBanding = ["Add banding", "Добавить кромку"],
    ChooseSegment = ["Choose a segment", "Выберите сегмент"],
    MachiningIssueOne = ["1 machining issue is shown with its operation.", "1 замечание по обработке показано у операции."],
    MachiningIssueFew = ["{} machining issues are shown with their operations.", "{} замечания по обработке показаны у операций."],
    MachiningIssueMany = ["{} machining issues are shown with their operations.", "{} замечаний по обработке показаны у операций."],
    NoIssues = ["No issues.", "Замечаний нет."],
    ThicknessRow = ["Thickness  {}", "Толщина: {}"],
    CoreRow = ["Core  {} × {}", "Сердцевина: {} × {}"],
    OverallRow = ["Overall  {} × {}", "Габарит: {} × {}"],
    GeometryError = ["Geometry: {}", "Геометрия: {}"],
    EdgeBottom = ["Bottom", "Нижняя"],
    EdgeRight = ["Right", "Правая"],
    EdgeTop = ["Top", "Верхняя"],
    EdgeLeft = ["Left", "Левая"],
    EdgeGeneric = ["Edge", "Кромка"],
    DiagNoMaterial = ["No panel material is assigned.", "Материал панели не назначен."],
    DiagPanelMaterialMissing = [
        "Panel material {} is missing from the catalogue.",
        "Материал панели {} отсутствует в каталоге."
    ],
    DiagEdgeMaterialMissing = [
        "Edge {} uses missing edge material {}.",
        "Кромка {} использует отсутствующий материал {}."
    ],
    DiagCornerMaterialMissing = [
        "Corner {} uses missing edge material {}.",
        "Угол {} использует отсутствующий материал {}."
    ],
    DiagSegmentMaterialMissing = [
        "Segment {} uses missing edge material {}.",
        "Сегмент {} использует отсутствующий материал {}."
    ],
    DiagNonPositiveCore = ["The panel core is not positive.", "Сердцевина панели не положительна."],
    DiagInvalidOutline = [
        "The panel outline or its edge banding does not form a valid contour.",
        "Контур панели или его кромка не образуют допустимый контур."
    ],
    DiagMachiningOutsideCore = ["Machining {} lies outside the core.", "Обработка {} выходит за сердцевину."],
    DiagCoreRemoved = ["The cutouts remove the whole panel core.", "Вырезы удаляют всю сердцевину панели."],
    DiagCoreDisconnected = [
        "The cutouts split the panel core into disconnected pieces.",
        "Вырезы разделяют сердцевину панели на несвязанные части."
    ],
    DiagBlindDepth = [
        "Machining {} blind depth {} is not less than thickness {}.",
        "Глубина {} обработки {} не меньше толщины {}."
    ],
    SegmentWithArc = ["Segment {} (arc)", "Сегмент {} (дуга)"],
    SegmentWith = ["Segment {}", "Сегмент {}"],
    CornerGeneric = ["Corner", "Угол"],
    CornerRadiusSuffix = [" (radius)", " (радиус)"],
    CornerChamferSuffix = [" (chamfer)", " (фаска)"],
    CornerWithShape = ["Corner {}{}", "Угол {}{}"],
    Included = ["Included", "Включено"],
    RemoveBanding = ["Remove banding", "Убрать кромку"],

    // ── Camera and light cards ──────────────────────────────────────────────
    SectionProjection = ["Projection", "Проекция"],
    Perspective = ["Perspective", "Перспективная"],
    Orthographic = ["Orthographic", "Ортогональная"],
    FieldOfView = ["Field of view", "Поле зрения"],
    BetweenRange = ["Between {} and {}.", "От {} до {}."],
    CameraHeightHint = ["2000 mm", "2000 мм"],
    PositiveAtMost = ["Positive and at most {}.", "Положительное и не больше {}."],
    SectionRenderView = ["Render view", "Вид рендера"],
    RenderFromCamera = ["Render the scene from this camera", "Рендерить сцену с этой камеры"],
    CameraNameEmpty = ["Camera name must not be empty.", "Имя камеры не должно быть пустым."],
    FieldOfViewError = ["Field of view: {}.", "Поле зрения: {}."],
    FovMustBeBetween = ["Field of view must be between {} and {}.", "Поле зрения должно быть от {} до {}."],
    OrthoHeightError = ["Orthographic height: {}.", "Ортогональная высота: {}."],
    OrthoHeightPositiveAtMost = [
        "Orthographic height must be positive and at most {}.",
        "Ортогональная высота должна быть положительной и не больше {}."
    ],
    LightNameEmpty = ["Light name must not be empty.", "Имя источника света не должно быть пустым."],
    SectionLight = ["Light", "Свет"],
    Sun = ["Sun", "Солнце"],
    Point = ["Point", "Точечный"],
    Colour = ["Colour", "Цвет"],
    PowerWatts = ["Power (W)", "Мощность (Вт)"],
    Intensity = ["Intensity", "Интенсивность"],
    Power = ["Power", "Мощность"],
    ColourComponents = ["Colour components must be between 0 and 1.", "Компоненты цвета должны быть от 0 до 1."],
    IntensityRange = ["{} must be between 0 and {}.", "{}: допустимо от 0 до {}."],


    // ── Semantic classes (ADR 0094) ──────────────────────────────────────────
    ClassFamilySeating = ["Seating", "Места для сидения"],
    ClassFamilyLying = ["Lying", "Места для лежания"],
    ClassFamilyStorage = ["Storage", "Хранение"],
    ClassFamilySurfaces = ["Surfaces", "Поверхности"],
    ClassFamilyFittings = ["Fittings", "Фурнитура"],
    ClassFamilyAppliances = ["Appliances", "Бытовая техника"],
    ClassFamilyBoardsTrim = ["Boards and trim", "Плиты и погонаж"],
    ClassFamilyGeneral = ["General", "Общее"],

    ClassChair = ["Chair", "Стул"],
    ClassStool = ["Stool", "Табурет"],
    ClassBench = ["Bench", "Скамья"],
    ClassSofa = ["Sofa", "Диван"],
    ClassBed = ["Bed", "Кровать"],
    ClassMattress = ["Mattress", "Матрас"],
    ClassCabinet = ["Cabinet", "Шкаф корпусный"],
    ClassBaseCabinet = ["Base cabinet", "Шкаф нижний"],
    ClassWallCabinet = ["Wall cabinet", "Шкаф навесной"],
    ClassTallCabinet = ["Tall cabinet", "Шкаф высокий"],
    ClassVerticalPanel = ["Vertical panel", "Вертикальная панель"],
    ClassHorizontalPanel = ["Horizontal panel", "Горизонтальная панель"],
    ClassBackPanel = ["Back panel", "Задняя стенка"],
    ClassDrawerBoxPart = ["Drawer box part", "Деталь ящика"],
    ClassGlassShelf = ["Glass shelf", "Стеклянная полка"],
    ClassTamburatPanel = ["Tamburat panel", "Плита тамбурат"],
    ClassBentPanel = ["Bent panel", "Гнутая панель"],
    ClassHingedDoorMade = ["Hinged door, made", "Дверь распашная, своя"],
    ClassHingedDoorBought = ["Hinged door, bought", "Дверь распашная, покупная"],
    ClassSlidingDoorMade = ["Sliding door, made", "Дверь раздвижная, своя"],
    ClassSlidingDoorBought = ["Sliding door, bought", "Дверь раздвижная, покупная"],
    ClassLiftUpMade = ["Lift-up front, made", "Откидной фасад, свой"],
    ClassLiftUpBought = ["Lift-up front, bought", "Откидной фасад, покупной"],
    ClassDrawerFrontMade = ["Drawer front, made", "Фасад ящика, свой"],
    ClassDrawerFrontBought = ["Drawer front, bought", "Фасад ящика, покупной"],
    ClassDrawerMade = ["Drawer, made", "Ящик, свой"],
    ClassDrawerBought = ["Drawer, bought", "Ящик, покупной"],
    ClassTable = ["Table", "Стол"],
    ClassFloorTable = ["Floor table", "Стол напольный"],
    ClassWallTable = ["Wall table", "Стол навесной"],
    ClassCardHinge = ["Card hinge", "Петля карточная"],
    ClassCardHingeOverlay = ["Card hinge, overlay", "накладная"],
    ClassCardHingeInset = ["Card hinge, inset", "вкладная"],
    ClassCardHingeHalf = ["Card hinge, half-overlay", "полунакладная"],
    ClassCupHinge = ["Cup hinge", "Петля чашечная"],
    ClassCupHingeOverlay = ["Cup hinge, overlay", "накладная"],
    ClassCupHingeInset = ["Cup hinge, inset", "вкладная"],
    ClassCupHingeHalf = ["Cup hinge, half-overlay", "полунакладная"],
    ClassRunner = ["Runner", "Направляющая"],
    ClassRunnerRoller = ["Roller runner", "роликовая"],
    ClassRunnerBall = ["Ball-bearing runner", "шариковая"],
    ClassRunnerUndermount = ["Undermount runner", "скрытая"],
    ClassHandle = ["Handle", "Ручка"],
    ClassCamConnector = ["Cam connector", "Эксцентрик"],
    ClassShelfPin = ["Shelf pin", "Полкодержатель"],
    ClassLiftMechanism = ["Lift mechanism", "Подъёмный механизм"],
    ClassLock = ["Lock", "Замок"],
    ClassLightProfile = ["Light profile", "Профиль подсветки"],
    ClassAccessory = ["Accessory", "Аксессуар"],
    ClassSlidingSystem = ["Sliding door system", "Система раздвижных дверей"],
    ClassSink = ["Sink", "Мойка"],
    ClassHob = ["Hob", "Варочная панель"],
    ClassOven = ["Oven", "Духовой шкаф"],
    ClassExtractor = ["Extractor", "Вытяжка"],
    ClassDishwasher = ["Dishwasher", "Посудомоечная машина"],
    ClassRefrigerator = ["Refrigerator", "Холодильник"],
    ClassWasher = ["Washer", "Стиральная машина"],
    ClassWorktop = ["Worktop", "Столешница"],
    ClassPlinth = ["Plinth", "Цоколь"],
    ClassCornice = ["Cornice", "Карниз"],
    ClassWallPanel = ["Wall panel", "Стеновая панель"],
    ClassEdgeProfile = ["Edge profile", "Кромочный профиль"],
    ClassHangingRail = ["Hanging rail", "Штанга"],
    ClassFiller = ["Filler strip", "Добор"],
    ClassGeneralProduct = ["General product", "Изделие общего назначения"],

    // ── Properties card ─────────────────────────────────────────────────────
    PropertiesTitle = ["Properties", "Свойства"],
    NoProperties = ["No properties.", "Нет свойств."],
    KeyHint = ["key", "ключ"],
    Add = ["Add", "Добавить"],
    Unclassified = ["unclassified", "без классификации"],
    PropertyKindText = ["Text", "Текст"],
    PropertyKindInteger = ["Integer", "Целое"],
    PropertyKindBool = ["Bool", "Логическое"],
    PropertyKindLength = ["Length", "Длина"],
    PropertyKindAngle = ["Angle", "Угол"],
    PropertyKindClass = ["Class", "Класс"],
    ClassPickerSearch = ["Search by name, code or alias", "Поиск по названию, коду или синониму"],
    ClassPickerNoMatch = ["No class matches.", "Нет подходящего класса."],
    ClassPickerNotInRegistry = ["not in the registry", "нет в реестре"],
    ClassPickerHint = ["Assign a class from the registry", "Назначить класс из реестра"],

    // ── Small shared strings ────────────────────────────────────────────────
    MeshVertexNoun = ["vertex", "вершина"],
    MeshFaceNoun = ["face", "грань"],
    UiScale = ["UI scale", "Масштаб интерфейса"],
    UiScaleHint = [
        "Icons are rasterised at the new pixel size instead of stretched",
        "Значки растрируются в новом размере пикселя, а не растягиваются"
    ],
    ExtrudeRefused = ["The selected path cannot be extruded: {}", "Выбранный путь нельзя выдавить: {}"],
    LockedNode = [
        "{} is locked. Unlock it in the scene tree to change it.",
        "{} заблокирован. Снимите блокировку в дереве сцены, чтобы изменить его."
    ],
    TheNode = ["The node", "Узел"],

    // ── Scene tree ──────────────────────────────────────────────────────────
    TreeBadgeComponent = ["component", "компонент"],
    TreeBadgeCamera = ["camera", "камера"],
    TreeBadgeLight = ["light", "свет"],
    TreeBadgeMesh = ["mesh", "сетка"],
    TreeBadgeContour = ["contour", "контур"],
    TreeBadgePath = ["path", "путь"],
    TreeBadgeCube = ["cube", "куб"],
    TreeBadgeCylinder = ["cylinder", "цилиндр"],
    TreeBadgeSphere = ["sphere", "сфера"],
    TreeGroupSelection = ["Group selection", "Сгруппировать выделение"],
    TreeAddChild = ["Add child", "Добавить дочерний"],
    TreeAddChildHint = ["Create the next node inside this container", "Следующий созданный узел попадёт в этот контейнер"],
    TreeShow = ["Show", "Показать"],
    TreeHide = ["Hide", "Скрыть"],
    TreeUnlock = ["Unlock", "Снять блокировку"],
    TreeLock = ["Lock", "Заблокировать"],
    TreeIsolate = ["Isolate", "Изолировать"],
    TreeIsolateHint = ["Hide every other branch of the scene", "Скрыть все остальные ветви сцены"],
    TreeShowAll = ["Show all", "Показать всё"],
    TreeShowAllHint = ["Show every hidden node again", "Снова показать все скрытые узлы"],
    TreeOpenProduct = ["Open the product", "Открыть изделие"],
    TreeCloseProduct = ["Close the product", "Закрыть изделие"],
    TreeActiveProduct = ["Open for parts", "Открыто для деталей"],

    // ── Product card ────────────────────────────────────────────────────────
    ProductCardTitle = ["New product frame", "Новый каркас изделия"],
    ProductCardHint = [
        "The frame is created open, ready for parts on the floor beside the other pieces.",
        "Каркас создаётся открытым, готовым для деталей на полу рядом с другими изделиями."
    ],
    Height = ["Height", "Высота"],

    // ── Container card ──────────────────────────────────────────────────────
    ContainerRoleComponent = ["Component", "Компонент"],
    ContainerRoleGroup = ["Group", "Группа"],
    ContainerComponentDescription = [
        "A furniture or industrial component. It keeps its own identity and must be converted to a group before Ungroup will flatten it.",
        "Мебельный или промышленный компонент. Он сохраняет свою идентичность, и перед разгруппировкой его нужно преобразовать в группу."
    ],
    ContainerGroupDescription = [
        "An organisational container with no furniture type. Ungroup flattens it into its parent without moving any child in world space.",
        "Организационный контейнер без мебельного типа. Разгруппировка переносит его содержимое в родителя, не сдвигая дочерние узлы в мировом пространстве."
    ],
    SelectNodeInTree = ["Select a node in the scene tree.", "Выберите узел в дереве сцены."],
    SectionName = ["Name", "Имя"],
    NameMustNotBeEmpty = ["{} name must not be empty.", "Имя «{}» не должно быть пустым."],
    SectionContainer = ["Container", "Контейнер"],
    RootDescription = [
        "Root; it defines the world frame and is never a furniture component.",
        "Корень; задаёт мировую систему координат и не является мебельным компонентом."
    ],
    ChildrenCount = ["Children  {}", "Дочерние узлы: {}"],
    SectionCreateInside = ["Create inside", "Создать внутри"],
    AddChildHint = ["The next created node lands in this container", "Следующий созданный узел попадёт в этот контейнер"],

    // ── Placement and pivot cards ───────────────────────────────────────────
    Origin = ["Origin", "Начало координат"],
    OriginLocalTo = ["Origin (local to {})", "Начало (относительно {})"],
    OriginLocalToParent = ["Origin (local to the parent)", "Начало (относительно родителя)"],
    OriginOutOfRange = [
        "The stored origin is outside the editable millimetre range.",
        "Сохранённое начало выходит за редактируемый диапазон миллиметров."
    ],
    SectionTransform = ["Transform", "Преобразование"],
    WorldOrigin = ["World origin  {}", "Мировое начало: {}"],
    WorldOriginOutOfRange = ["World origin  outside the editable range", "Мировое начало: вне редактируемого диапазона"],
    RotationWithOrder = ["Rotation ({})", "Поворот ({})"],
    Apply = ["Apply", "Применить"],
    NodeNotInScene = ["node {} is not in the scene", "узел {} отсутствует в сцене"],
    FieldWithError = ["{}: {}", "{}: {}"],
    RotateAxis = ["Rotate {}", "Поворот {}"],
    LengthErrorEmpty = ["enter a length in millimetres", "введите длину в миллиметрах"],
    LengthErrorInvalid = ["not a valid millimetre value", "недопустимое значение в миллиметрах"],
    LengthErrorTooPrecise = [
        "use at most three decimal places in millimetres",
        "не больше трёх знаков после запятой в миллиметрах"
    ],
    LengthErrorOutOfRange = ["length is outside the supported range", "длина выходит за поддерживаемый диапазон"],
    DegreesErrorEmpty = ["enter an angle in degrees", "введите угол в градусах"],
    DegreesErrorInvalid = ["not a valid degree value", "недопустимое значение в градусах"],
    DegreesErrorTooPrecise = [
        "use at most three decimal places in degrees",
        "не больше трёх знаков после запятой в градусах"
    ],
    DegreesErrorOutOfRange = ["angle is outside the supported range", "угол выходит за поддерживаемый диапазон"],
    NumberInvalid = ["enter a number", "введите число"],
    Pivot = ["Pivot", "Пивот"],
    PivotLocalOffset = ["Pivot (local offset)", "Пивот (локальное смещение)"],
    PanelPivotFollowsOrigin = ["Pivot follows origin (offset 0)", "Пивот следует за началом (смещение 0)"],
    PivotWorld = ["Pivot (world)  {}", "Пивот (мир): {}"],
    PivotWorldOutOfRange = ["Pivot (world)  outside the editable range", "Пивот (мир): вне редактируемого диапазона"],
    PivotInNodeFrame = ["Offset from the node's origin", "Смещение от начала узла"],
    PivotNotEditable = ["The stored pivot is not editable.", "Сохранённый пивот нельзя редактировать."],
    Center = ["Center", "Центр"],

    // ── Home ────────────────────────────────────────────────────────────────
    HomeClose = ["Close  (Esc)", "Закрыть  (Esc)"],
    HomeStartScene = ["Start a scene", "Начните сцену"],
    HomeNewScene = ["New scene", "Новая сцена"],
    HomeOpenScene = ["Open scene…", "Открыть сцену…"],
    HomeRecentFiles = ["Recent files", "Недавние файлы"],
    HomeNoScene = ["No scene opened yet.", "Пока нет открытых сцен."],
    HomeEmptyTemplate = ["Empty template", "Пустой шаблон"],
    HomeEmptyTemplateHint = [
        "A fresh scene with the default catalogue and nothing else.",
        "Новая сцена со стандартным каталогом и больше ничем."
    ],
    HomeUseTemplate = ["Use template", "Взять шаблон"],
    HomeUnnamed = ["(unnamed)", "(без имени)"],

    // ── Command palette ─────────────────────────────────────────────────────
    PaletteHeading = ["Commands", "Команды"],
    PaletteSearch = ["Search commands…", "Поиск команд…"],
    PaletteNoMatch = ["No command matches that search.", "Ни одна команда не подходит под запрос."],
    PaletteCountOne = ["{} command", "{} команда"],
    PaletteCountFew = ["{} commands", "{} команды"],
    PaletteCountMany = ["{} commands", "{} команд"],
    PaletteUnknownCommand = ["Unknown command `{}`.", "Неизвестная команда `{}`."],
    PaletteNotAllowed = ["The current selection does not allow this.", "Текущее выделение не позволяет это."],
    PaletteNeedsSelection = ["Needs a selection.", "Нужно выделение."],
    PaletteNeedsNewName = [
        "Needs a new name; rename the node in the inspector.",
        "Нужно новое имя; переименуйте узел в инспекторе."
    ],
    PaletteNeedsTwoNodes = ["Needs two or more selected nodes.", "Нужно два или больше выделенных узла."],
    PaletteNeedsCamera = ["Needs a selected camera.", "Нужна выделенная камера."],
    PaletteAddPanelMaterial = [
        "Add a panel material to the catalogue first.",
        "Сначала добавьте материал панели в каталог."
    ],
    GeneratedGroup = ["Group", "Группа"],

    // ── Drawing tools ───────────────────────────────────────────────────────
    ToolLine = ["Line", "Линия"],
    ToolPolyline = ["Polyline", "Полилиния"],
    ToolArc = ["Arc", "Дуга"],
    ToolArcFromCentre = ["Arc from centre", "Дуга от центра"],
    ToolRectangle = ["Rectangle", "Прямоугольник"],
    ToolRectangleFromCentre = ["Rectangle from centre", "Прямоуг.центр"],
    ToolRectangleFromCentreDirected = ["Directed rectangle", "Прямоуг.ось"],
    ToolCircle = ["Circle", "Окружность"],
    ToolCircleThreePoints = ["Circle through 3 points", "Окружн.3 точки"],
    ToolRegularPolygon = ["Polygon", "Многоугольник"],
    DrawingName = ["{} drawing", "{} — эскиз"],
    PolylineStripLabel = [
        "Polyline · Tab line/arc · Alt ignores snaps",
        "Полилиния · Tab линия/дуга · Alt игнорирует привязки"
    ],

    // ── Ribbon ──────────────────────────────────────────────────────────────
    RibbonFurniture = ["Furniture", "Мебель"],
    RibbonHardware = ["Hardware", "Фурнитура"],
    RibbonModel = ["Model", "Модель"],
    RibbonSketch = ["Sketch", "Эскиз"],
    RibbonScene = ["Scene", "Сцена"],
    RibbonView = ["View", "Вид"],

    // The ribbon's groups, in the order the map draws them (U68).
    GroupCreate = ["Create", "Создать"],
    GroupModify = ["Modify", "Изменить"],
    GroupConstruction = ["Construction", "Опорные"],
    GroupDimensions = ["Dimensions", "Размеры"],
    GroupBooleans = ["Booleans", "Булевы"],
    GroupDraft = ["Draft", "Черновик"],
    GroupPrimitives = ["Primitives", "Примитивы"],
    GroupFaces = ["Faces", "Грани"],
    GroupBodies = ["Bodies", "Тела"],
    GroupProduct = ["Product", "Изделие"],
    GroupPanels = ["Panels", "Панели"],
    GroupMachining = ["Machining", "Обработка"],
    GroupFasteners = ["Fasteners", "Крепёж"],
    GroupHardware = ["Hardware", "Фурнитура"],
    GroupChecks = ["Checks", "Проверка"],
    FillNeedsProduct = ["Open a product first", "Сначала откройте изделие"],
    FillNeedsMaterial = ["Add a panel material first", "Сначала добавьте материал панели"],
    FillNeedsFillTrait = ["This class does not fill a void", "Этот класс не заполняет пустоту"],
    GroupObjects = ["Objects", "Объекты"],
    GroupViews = ["Views", "Виды"],
    GroupMaterials = ["Materials", "Материалы"],
    GroupProjection = ["Projection", "Проекция"],
    GroupDisplay = ["Display", "Отображение"],
    GroupLayout = ["Layout", "Раскладка"],
    GroupNavigation = ["Navigation", "Навигация"],

    // The ribbon's tools (U68); a planned row is named here before its command exists.
    ToolProductFrame = ["Product frame", "Каркас изделия"],
    ToolPanel = ["Panel", "Панель"],
    ToolPost = ["Post", "Стойка"],
    ToolShelf = ["Shelf", "Полка"],
    ToolContourPanel = ["Panel from contour", "Панель по контуру"],
    ToolAssembly = ["Assembly", "Сборка"],
    ToolGroupNode = ["Group", "Группа"],
    ToolCamera = ["Camera", "Камера"],
    ToolLight = ["Light", "Свет"],
    ToolNamedView = ["Named view", "Именованный вид"],
    ToolStandardView = ["Standard view", "Стандартный вид"],
    ToolMaterialCatalogue = ["Material catalogue", "Каталог материалов"],
    ToolAssignMaterial = ["Assign material", "Назначить материал"],
    ToolFrameAll = ["Frame all", "Показать всё"],
    ToolProjection = ["Projection", "Проекция"],
    ToolGridSnap = ["Grid snap", "Привязка"],
    ToolStyle = ["Display style", "Стиль отображения"],
    ToolContours = ["Contours", "Контуры"],
    ToolShadows = ["Shadows", "Тени"],
    ToolSection = ["Section", "Сечение"],
    ToolLayoutOne = ["One view", "Один вид"],
    ToolLayoutTwo = ["Two views", "Два вида"],
    ToolLayoutFour = ["Four views", "Четыре вида"],
    ToolMaximize = ["Maximize the view", "Развернуть вид"],
    ToolOrbit = ["Orbit", "Орбита"],
    ToolPan = ["Pan", "Панорама"],
    ToolZoom = ["Zoom", "Масштаб"],
    SwitchProjection = ["Switch the projection", "Переключить проекцию"],
    ToolOrthographic = ["Orthographic", "Ортогональная"],
    ToolPerspective = ["Perspective", "Перспективная"],
    ToolExtrude = ["Extrude", "Выдавить"],
    ToolRevolve = ["Revolve", "Вращение"],
    ToolLoft = ["Loft", "По сечениям"],
    ToolSweep = ["Sweep", "По траектории"],
    ToolCube = ["Cube", "Куб"],
    ToolCylinder = ["Cylinder", "Цилиндр"],
    ToolSphere = ["Sphere", "Сфера"],
    ToolCone = ["Cone", "Конус"],
    ToolPyramid = ["Pyramid", "Пирамида"],
    ToolUnion = ["Union", "Объединить"],
    ToolSubtract = ["Subtract", "Вычесть"],
    ToolIntersect = ["Intersect", "Пересечение"],
    ToolFillet = ["Fillet", "Скругление"],
    ToolChamfer = ["Chamfer", "Фаска"],
    ToolShell = ["Shell", "Оболочка"],
    ToolOffsetFace = ["Offset face", "Сместить грань"],
    ToolDeleteFace = ["Delete face", "Удалить грань"],
    ToolJoin = ["Join", "Соединить"],
    ToolSplit = ["Split", "Разделить"],
    ToolScale = ["Scale", "Масштаб"],
    ToolMirror = ["Mirror", "Зеркало"],
    ToolTrim = ["Trim", "Обрезать"],
    ToolExtend = ["Extend", "Удлинить"],
    ToolOffset = ["Offset", "Сместить"],
    ToolBreak = ["Break", "Разорвать"],
    ToolConstructionHorizontal = ["Horizontal reference", "Опорная горизонталь"],
    ToolConstructionVertical = ["Vertical reference", "Опорная вертикаль"],
    ToolConstructionParallel = ["Parallel reference", "Опорная параллель"],
    ToolConstructionAngle = ["Angled reference", "Опорная под углом"],
    ToolDimensionLinear = ["Linear dimension", "Линейный размер"],
    ToolDimensionAngular = ["Angular dimension", "Угловой размер"],
    ToolDimensionRadius = ["Radial dimension", "Радиальный размер"],
    ToolDrill = ["Drill", "Сверление"],
    ToolGroove = ["Groove", "Паз"],
    ToolEdgeband = ["Edge banding", "Кромка"],
    ToolHingeHole = ["Hinge hole", "Отверстие петли"],
    ToolConfirmat = ["Confirmat", "Конфирмат"],
    ToolCam = ["Cam lock", "Эксцентрик"],
    ToolDowel = ["Dowel", "Шкант"],
    ToolMinifix = ["Minifix", "Минификс"],
    ToolShelfPin = ["Shelf pin", "Полкодержатель"],
    ToolHinge = ["Hinge", "Петля"],
    ToolRunner = ["Runner", "Направляющая"],
    ToolHandle = ["Handle", "Ручка"],
    ToolFoot = ["Foot", "Опора"],
    ToolCollisions = ["Collisions", "Пересечения"],
    ToolPartsList = ["Parts list", "Список деталей"],
    ToolCutList = ["Cut list", "Раскрой"],

    // The ribbon's variants: the input methods of one tool (U67).
    ToolCircleTwoPoints = ["Through 2 points", "По 2 точкам"],
    ToolCircleTangent = ["Tangent to two lines", "Касательно двух линий"],
    ToolArcThreePoints = ["Through 3 points", "По 3 точкам"],
    ToolLineAngle = ["By angle", "Под углом"],
    ToolPolygonEdge = ["By edge", "По ребру"],
    ToolOffsetDistance = ["By distance", "На расстояние"],
    ToolOffsetThrough = ["Through a point", "Через точку"],
    ToolFilletRadius = ["By radius", "По радиусу"],
    ToolFilletTwoLines = ["Two lines", "Две линии"],
    ToolChamferDistance = ["By distance", "На расстояние"],
    ToolChamferTwoLines = ["Two lines", "Две линии"],
    ToolDiameter = ["Diameter", "Диаметр"],
    ToolCircleCentreRadius = ["Centre and radius", "Центр и радиус"],
    ToolArcTwoPoints = ["Through 2 points", "По 2 точкам"],
    ToolRectangleFromCorner = ["From corner", "От угла"],
    ToolLineTwoPoints = ["Two points", "По двум точкам"],
    ToolPolygonSides = ["By side count", "По числу сторон"],
    SectionBuild = ["Build", "Построение"],
    SectionTangents = ["Tangents", "Касательные"],
    PinMethod = ["Use this as the main method", "Сделать основным способом"],
    MethodPinned = ["The main method", "Основной способ"],
    ToolMethods = ["All methods", "Все способы"],

    FieldSides = ["Sides", "Стороны"],
    FieldSize = ["Size", "Размер"],
    FieldHeight = ["Height", "Высота"],
    Close = ["Close", "Закрыть"],
    Cancel = ["Cancel", "Отмена"],
    DraftCloseHint = ["Close the draft into a ring", "Замкнуть черновик в кольцо"],
    DraftCloseNeedsTwo = ["A draft needs two segments to close", "Чтобы замкнуть черновик, нужно два сегмента"],
    DraftCancelHint = ["Cancel the draft", "Отменить черновик"],
    DraftNotRunning = ["No draft is running", "Черновик не запущен"],
    ExtrudeSelectPath = ["Select a closed path first", "Сначала выберите замкнутый путь"],
    ExtrudeSelectedPath = ["Extrude the selected path", "Выдавить выбранный путь"],
    ExtrudePathOpen = ["The selected path is open; close it first", "Выбранный путь разомкнут; сначала замкните его"],
    ExtrudeNotPath = ["The selected object is not a path", "Выбранный объект не является путём"],
    PrimitiveClickPlane = ["{}: click the work plane", "{}: щёлкните по рабочей плоскости"],

    // ── Display names ───────────────────────────────────────────────────────
    Axonometric = ["Axonometric", "Аксонометрия"],
    Front = ["Front", "Спереди"],
    Left = ["Left", "Слева"],
    Top = ["Top", "Сверху"],
    Right = ["Right", "Справа"],
    Bottom = ["Bottom", "Снизу"],
    ShadingEdgesCaption = ["Edges", "Рёбра"],
    ShadingShadedCaption = ["Shaded", "Заливка"],
    ShadingWireCaption = ["Wire", "Каркас"],
    ShadingHiddenCaption = ["Hidden", "Скрытые"],
    ShadingGhostCaption = ["Ghost", "Призрак"],
    ShadingShadedWithEdges = ["Shaded with edges", "Заливка с рёбрами"],
    ShadingShaded = ["Shaded", "Заливка"],
    ShadingWireframe = ["Wireframe", "Каркас"],
    ShadingHiddenEdges = ["Shaded with hidden edges", "Заливка со скрытыми рёбрами"],
    ShadingTranslucent = ["Translucent", "Полупрозрачный"],

    // ── Workspace settings ──────────────────────────────────────────────────
    SettingsTitle = ["Settings", "Настройки"],
    Done = ["Done", "Готово"],
    SectionAppearance = ["Appearance", "Оформление"],
    SectionViewport = ["Viewport", "Область просмотра"],
    SectionPanels = ["Panels", "Панели"],
    SectionShortcuts = ["Shortcuts", "Горячие клавиши"],
    Theme = ["Theme", "Тема"],
    ThemeDark = ["Dark", "Тёмная"],
    ThemeLight = ["Light", "Светлая"],
    ThemeSystem = ["System", "Системная"],
    Palette = ["Palette", "Палитра"],
    PaletteGraphiteCopper = ["Graphite · copper", "Графит · медь"],
    PaletteInkAzure = ["Ink · azure", "Чернила · лазурь"],
    PaletteSlateMint = ["Slate · mint", "Сланец · мята"],
    PaletteHint = [
        "A preset paints the whole window in both variants; the accent is the one colour you can replace.",
        "Палитра окрашивает всё окно в обоих вариантах; акцент — единственный цвет, который можно заменить."
    ],
    Accent = ["Accent", "Акцент"],
    AccentHint = [
        "One accent for the dark and the light window; Reset returns to the palette's own.",
        "Один акцент для тёмного и светлого окна; сброс возвращает акцент палитры."
    ],
    Language = ["Language", "Язык"],
    LanguageEnglish = ["English", "English"],
    LanguageRussian = ["Русский", "Русский"],
    ScaleHint = [
        "The scale applies to the whole window; icons are rasterised at the new pixel size.",
        "Масштаб применяется ко всему окну; значки растрируются в новом размере."
    ],
    DisplayStyle = ["Display style", "Стиль отображения"],
    OpeningView = ["Opening view", "Начальный вид"],
    RollRingStep = ["Roll ring step", "Шаг кольца крена"],
    RollRingStepHint = [
        "Degrees per step when the ring under the corner widget is turned by hand.",
        "Градусов за шаг при ручном повороте кольца под виджетом ориентации."
    ],
    ShowFloorGrid = ["Show the floor grid", "Показывать сетку пола"],
    ShowEveryTriangleEdge = ["Show every triangle edge", "Показывать все рёбра треугольников"],
    AllEdgesHint = [
        "Diagnostic: keeps the triangulation diagonals the form-edge view hides.",
        "Диагностика: оставляет диагонали триангуляции, которые скрывает вид форменных рёбер."
    ],
    ViewportDefaultsHint = [
        "These are the defaults of a new viewport; the open view keeps its own state.",
        "Это настройки нового вида; открытый вид сохраняет своё состояние."
    ],
    PanelOpacity = ["Panel opacity", "Непрозрачность панелей"],
    PanelOpacityHint = [
        "Lower values let the scene show through the chrome drawn over it.",
        "Меньшие значения позволяют сцене просвечивать сквозь панели над ней."
    ],
    ScenePanelVisible = ["Scene panel", "Панель сцены"],
    InspectorPanelVisible = ["Inspector panel", "Панель инспектора"],
    ResetPanels = ["Reset panels", "Сбросить панели"],
    PanelsHint = [
        "Panels float over the scene; the header buttons collapse or hide both at once.",
        "Панели плавают над сценой; кнопки в шапке сворачивают или скрывают обе сразу."
    ],
    ShortcutHidePanels = ["Hide all panels", "Скрыть все панели"],
    ShortcutCollapsePanels = ["Collapse all panels", "Свернуть все панели"],
    IconColor = ["Icon colour", "Цвет значков"],
    IconColorAuto = ["Automatic", "Автоматически"],
    IconColorAlways = ["Always", "Всегда"],
    IconColorNever = ["Never", "Никогда"],
    IconColorHint = [
        "Icons take their hue from the semantic family; Automatic falls back to monochrome below a 20 px raster.",
        "Значки берут оттенок из смыслового семейства; «Автоматически» переходит в монохромный при растре меньше 20 px."
    ],
    ResetAppearance = ["Reset", "Сбросить"],
    ResetAppearanceHint = [
        "Restores the default palette, accent, opacity and icon colour.",
        "Возвращает палитру, акцент, прозрачность и цвет значков по умолчанию."
    ],
    ShortcutsHint = [
        "The same keys the ribbon, the header and the status bar name.",
        "Те же клавиши, что названы на ленте, в заголовке и в строке состояния."
    ],
    ShortcutNewScene = ["Open an empty scene in a new tab", "Открыть пустую сцену в новой вкладке"],
    ShortcutCloseTab = ["Close the active tab", "Закрыть активную вкладку"],
    ShortcutSave = ["Save the scene", "Сохранить сцену"],
    ShortcutUndo = ["Undo", "Отменить"],
    ShortcutRedo = ["Redo", "Повторить"],
    ShortcutPalette = ["Command palette", "Палитра команд"],
    ShortcutFrameAll = ["Frame all", "Показать всё"],
    ShortcutProjection = ["Perspective / orthographic", "Перспектива / ортогональ"],
    ShortcutExpand = ["Expand / restore the active view", "Развернуть / восстановить активный вид"],
    ShortcutGrid = ["Toggle the drawing grid", "Переключить сетку рисования"],
    ShortcutCancel = ["Cancel the current tool", "Отменить текущий инструмент"],
    ShortcutOrbit = ["Orbit", "Облёт"],
    ShortcutPan = ["Pan", "Панорама"],
    ShortcutZoom = ["Zoom", "Масштаб"],

    // ── Commands (furnicraft-engine) ────────────────────────────────────────
    CmdCreateCamera = ["Create camera", "Создать камеру"],
    CmdCreateLight = ["Create light", "Создать источник света"],
    CmdSetActiveCamera = ["Set active camera", "Сделать камеру активной"],
    CmdSetCameraProjection = ["Set camera projection", "Задать проекцию камеры"],
    CmdSetLightParameters = ["Set light parameters", "Задать параметры света"],
    CmdCreateAssembly = ["Create assembly", "Создать сборку"],
    CmdCreateProduct = ["Create product frame", "Создать каркас изделия"],
    CreationNeedsProduct = ["Create a product first.", "Сначала создайте изделие."],
    CreationProductClosed = [
        "The product is closed; open it before adding a part.",
        "Изделие закрыто; откройте его, прежде чем добавлять деталь."
    ],
    CmdSetProductEnvelope = ["Set product frame size", "Задать габарит изделия"],
    CmdRename = ["Rename", "Переименовать"],
    CmdDelete = ["Delete", "Удалить"],
    CmdDuplicate = ["Duplicate", "Дублировать"],
    CmdGroup = ["Group", "Сгруппировать"],
    CmdUngroup = ["Ungroup", "Разгруппировать"],
    CmdPlace = ["Place", "Разместить"],
    CmdPlaceAtWorldZero = ["Place at world zero", "На начало мира"],
    CmdPlaceSelection = ["Place selection", "Разместить выделение"],
    CmdSetPivot = ["Set pivot", "Задать пивот"],
    CmdCenterPivot = ["Center pivot", "Центрировать пивот"],
    CmdRebaseOrigin = ["Rebase origin", "Переякорить начало"],
    CmdSetProperty = ["Set property", "Задать свойство"],
    CmdSetProperties = ["Set properties", "Задать свойства"],
    CmdClearProperty = ["Clear property", "Очистить свойство"],
    CmdConvertToGroup = ["Convert to group", "Преобразовать в группу"],
    CmdMoveVertices = ["Move vertices", "Переместить вершины"],
    CmdCreatePrimitive = ["Create primitive", "Создать примитив"],
    CmdReplacePrimitive = ["Replace primitive parameters", "Заменить параметры примитива"],
    CmdCreateExtrusion = ["Create extrusion", "Создать выдавливание"],
    CmdReplaceContour = ["Replace contour", "Заменить контур"],
    CmdExtrudePath = ["Extrude path", "Выдавить путь"],
    CmdSetExtrusionHeight = ["Set extrusion height", "Задать высоту выдавливания"],
    CmdCreatePath = ["Create path", "Создать путь"],
    CmdReplacePath = ["Replace path", "Заменить путь"],
    CmdCollapseToMesh = ["Collapse to mesh", "Свернуть в сетку"],
    CmdSetEdgeBanding = ["Set edge banding", "Задать кромку"],
    CmdClearEdgeBanding = ["Clear edge banding", "Убрать кромку"],
    CmdSetCornerBanding = ["Set corner banding", "Задать кромку угла"],
    CmdClearCornerBanding = ["Clear corner banding", "Убрать кромку угла"],
    CmdSetContourBanding = ["Set contour banding", "Задать кромку контура"],
    CmdClearContourBanding = ["Clear contour banding", "Убрать кромку контура"],
    CmdSetCornerFeature = ["Set corner feature", "Задать обработку угла"],
    CmdClearCornerFeature = ["Clear corner feature", "Убрать обработку угла"],
    CmdSetEdgeBow = ["Set edge bow", "Задать прогиб кромки"],
    CmdClearEdgeBow = ["Clear edge bow", "Убрать прогиб кромки"],
    CmdAddMachining = ["Add machining", "Добавить обработку"],
    CmdUpdateMachining = ["Update machining", "Изменить обработку"],
    CmdRemoveMachining = ["Remove machining", "Удалить обработку"],
    CmdMoveMachining = ["Move machining", "Переместить обработку"],
    CmdCreatePanel = ["Create panel", "Создать панель"],
    CmdCreateContourPanel = ["Create contour panel", "Создать панель по контуру"],
    CmdSetPanelOutline = ["Set panel outline", "Задать контур панели"],
    CmdBakePanelOutline = ["Bake panel outline into a contour", "Преобразовать контур панели в кривую"],
    CmdRenamePanel = ["Rename panel", "Переименовать панель"],
    CmdResizePanel = ["Resize panel", "Изменить размер панели"],
    CmdSetPanelMaterial = ["Set panel material", "Задать материал панели"],
}

impl Message {
    /// "1 point / 2 points / 5 points", a counted message the tests exercise.
    pub const fn test_points(n: usize) -> crate::Counted {
        Self::counted(n, Self::TestPointOne, Self::TestPointFew, Self::TestPointMany)
    }
}
