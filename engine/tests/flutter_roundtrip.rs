use translatecode_engine::frontend::dart::DartFrontend;
use translatecode_engine::frontend::Frontend;
use translatecode_engine::{translate, Language};

const FLUTTER_APP: &str = include_str!("fixtures/flutter_roundtrip.dart");

#[test]
fn flutter_dependency_graph_survives_dart_python_dart_round_trip() {
    let python = translate(FLUTTER_APP, Language::Dart, Language::Python);
    let python_unit = translatecode_engine::frontend::parse_source(&python, Language::Python);
    assert!(
        python_unit.diagnostics.is_empty(),
        "{:#?}\n{}",
        python_unit.diagnostics,
        python
    );
    assert!(python.contains("async def _initializeDependencies() -> None:"));
    assert!(python.contains("Dio(BaseOptions(baseUrl=AppConstants.baseUrl"));
    assert!(python.contains("Get.put[NewsRepository](newsRepository)"));
    assert!(python.contains("class FusionNewsApp(StatelessWidget):"));
    assert!(python.contains("def _buildTheme(self) -> ThemeData:"));
    assert!(python.contains("def _getPages(self) -> list[GetPage]:"));
    assert!(
        !python.contains("baseUrl:"),
        "Dart named argument leaked into Python"
    );
    assert!(
        !python.contains("const FusionNewsApp"),
        "Dart const leaked into Python"
    );

    let dart = translate(&python, Language::Python, Language::Dart);
    let dart_unit = DartFrontend.parse(&dart);
    assert!(
        dart_unit
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.severity
                != translatecode_engine::diagnostic::Severity::Error),
        "{:#?}\n{}",
        dart_unit.diagnostics,
        dart
    );
    assert_eq!(dart_unit.imports.len(), 21, "{}", dart);
    for retained in [
        "Future<void> _initializeDependencies() async",
        "Dio(BaseOptions(baseUrl: AppConstants.baseUrl",
        "NewsRepositoryImpl(remoteDataSource: newsRemoteDataSource",
        "Get.put<NewsRepository>(newsRepository)",
        "class FusionNewsApp extends StatelessWidget",
        "Widget build(BuildContext context)",
        "ThemeData _buildTheme()",
        "List<GetPage> _getPages()",
        "final article = Get.arguments;",
        "Future<void> main() async",
    ] {
        assert!(dart.contains(retained), "missing `{}`:\n{}", retained, dart);
    }
    assert!(!dart.contains("Dio(;"), "{}", dart);
    assert!(!dart.contains("pass;"), "{}", dart);
}
