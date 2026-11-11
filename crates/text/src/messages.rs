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



    // ── Drawing tools ───────────────────────────────────────────────────────
    ToolLine = ["Line", "Линия"],
    ToolPolyline = ["Polyline", "Полилиния"],
    ToolArc = ["Arc", "Дуга"],
    ToolArcFromCentre = ["Arc from centre", "Дуга от центра"],
    ToolRectangle = ["Rectangle", "Прямоугольник"],
    ToolRectangleFromCentre = ["Rectangle from centre", "Прямоугольник от центра"],
    ToolRectangleFromCentreDirected = ["Directed rectangle", "Направленный прямоугольник"],
    ToolCircle = ["Circle", "Окружность"],
    ToolCircleThreePoints = ["Circle through 3 points", "Окружность через 3 точки"],
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
    GroupDraw = ["Draw", "Рисование"],
    GroupShapes = ["Shapes", "Фигуры"],
    GroupDraft = ["Draft", "Черновик"],
    GroupPrimitives = ["Primitives", "Примитивы"],
    GroupBodies = ["Bodies", "Тела"],
    GroupScene = ["Scene", "Сцена"],
    GroupView = ["View", "Вид"],
    GroupDisplay = ["Display", "Отображение"],
    ToolPanel = ["Panel", "Панель"],
    ToolAssembly = ["Assembly", "Сборка"],
    ToolCamera = ["Camera", "Камера"],
    ToolLight = ["Light", "Свет"],
    ToolFrameAll = ["Frame all", "Показать всё"],
    ToolGridSnap = ["Grid snap", "Привязка к сетке"],
    SwitchProjection = ["Switch the projection", "Переключить проекцию"],
    ToolOrthographic = ["Orthographic", "Ортогональная"],
    ToolPerspective = ["Perspective", "Перспективная"],
    ToolExtrude = ["Extrude", "Выдавить"],
    ToolCube = ["Cube", "Куб"],
    ToolCylinder = ["Cylinder", "Цилиндр"],
    ToolSphere = ["Sphere", "Сфера"],
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
    PrimitiveSizeNotLength = ["The primitive size is not a length: {}", "Размер примитива не является длиной: {}"],
    PrimitiveSizePositive = ["The primitive size must be positive.", "Размер примитива должен быть положительным."],
    ExtrudeHeightNotLength = ["The extrusion height is not a length: {}", "Высота выдавливания не является длиной: {}"],
    ExtrudeHeightPositive = ["The extrusion height must be positive.", "Высота выдавливания должна быть положительной."],
    ExtrudeHeightMax = ["The extrusion height must be at most {} mm.", "Высота выдавливания должна быть не больше {} мм."],

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
    SectionRibbon = ["Ribbon", "Лента"],
    SectionShortcuts = ["Shortcuts", "Горячие клавиши"],
    Theme = ["Theme", "Тема"],
    ThemeDark = ["Dark", "Тёмная"],
    ThemeLight = ["Light", "Светлая"],
    ThemeSystem = ["System", "Системная"],
    Accent = ["Accent", "Акцент"],
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
    BackgroundOpacity = ["Background opacity", "Непрозрачность фона"],
    RibbonOpacityHint = [
        "Lower values let the scene under the ribbon show through its body.",
        "Меньшие значения позволяют сцене под лентой просвечивать сквозь неё."
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
    CmdRename = ["Rename", "Переименовать"],
    CmdDelete = ["Delete", "Удалить"],
    CmdDuplicate = ["Duplicate", "Дублировать"],
    CmdGroup = ["Group", "Сгруппировать"],
    CmdUngroup = ["Ungroup", "Разгруппировать"],
    CmdPlace = ["Place", "Разместить"],
    CmdZeroTranslation = ["Zero translation", "Обнулить перенос"],
    CmdPlaceSelection = ["Place selection", "Разместить выделение"],
    CmdSetPivot = ["Set pivot", "Задать пивот"],
    CmdCenterPivot = ["Center pivot", "Центрировать пивот"],
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
