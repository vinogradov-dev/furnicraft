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
    Placement = ["Placement", "Размещение"],
    TranslationLocalTo = ["Translation (local to {})", "Перенос (относительно {})"],
    TranslationLocalToParent = ["Translation (local to the parent)", "Перенос (относительно родителя)"],
    TranslationOutOfRange = [
        "The stored translation is outside the editable millimetre range.",
        "Сохранённый перенос выходит за редактируемый диапазон миллиметров."
    ],
    SectionTransform = ["Transform", "Преобразование"],
    SectionTranslation = ["Translation", "Перенос"],
    WorldPosition = ["World position  {}", "Мировая позиция: {}"],
    WorldPositionOutOfRange = ["World position  outside the editable range", "Мировая позиция: вне редактируемого диапазона"],
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
    Pivot = ["Pivot", "Пивот"],
    PivotDerived = ["Derived from the panel's geometry", "Выводится из геометрии панели"],
    SectionLocalPivot = ["Local pivot", "Локальный пивот"],
    ZeroMillimetres = ["0 mm", "0 мм"],
    PivotBaseCorner = ["Base corner of the panel.", "Базовый угол панели."],
    PivotWorld = ["Pivot (world)  {}", "Пивот (мир): {}"],
    PivotWorldOutOfRange = ["Pivot (world)  outside the editable range", "Пивот (мир): вне редактируемого диапазона"],
    PivotInNodeFrame = ["In the node's own frame", "В собственной системе узла"],
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
