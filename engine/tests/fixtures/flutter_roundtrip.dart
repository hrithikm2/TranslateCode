import 'package:flutter/material.dart';
import 'package:get/get.dart';
import 'package:dio/dio.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'core/constants/app_constants.dart';
import 'core/network/network_info.dart';
import 'core/utils/app_routes.dart';
import 'features/news/data/datasources/news_remote_datasource.dart';
import 'features/news/data/datasources/news_local_datasource.dart';
import 'features/news/data/repositories/news_repository_impl.dart';
import 'features/news/domain/repositories/news_repository.dart';
import 'features/news/domain/entities/news_article.dart';
import 'features/news/presentation/controllers/news_controller.dart';
import 'features/livestream/data/datasources/agora_service.dart';
import 'features/livestream/data/repositories/live_stream_repository_impl.dart';
import 'features/livestream/domain/repositories/live_stream_repository.dart';
import 'features/livestream/presentation/controllers/live_stream_controller.dart';
import 'features/shared/presentation/pages/home_page.dart';
import 'features/news/presentation/pages/news_reel_page.dart';
import 'features/news/presentation/pages/news_detail_page.dart';
import 'features/livestream/presentation/pages/live_stream_page.dart';

/// Main entry point of the Fusion News application.
void main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await _initializeDependencies();
  runApp(const FusionNewsApp());
}

/// Initialize all app dependencies.
Future<void> _initializeDependencies() async {
  final sharedPreferences = await SharedPreferences.getInstance();
  final dio = Dio(
    BaseOptions(
      baseUrl: AppConstants.baseUrl,
      connectTimeout: Duration(milliseconds: AppConstants.connectionTimeout),
      receiveTimeout: Duration(milliseconds: AppConstants.receiveTimeout),
    ),
  );
  final networkInfo = NetworkInfoImpl();
  final agoraService = AgoraService();
  final newsRemoteDataSource = NewsRemoteDataSourceImpl(dio: dio);
  final newsLocalDataSource = NewsLocalDataSourceImpl(prefs: sharedPreferences);
  final newsRepository = NewsRepositoryImpl(
    remoteDataSource: newsRemoteDataSource,
    localDataSource: newsLocalDataSource,
    networkInfo: networkInfo,
  );
  final liveStreamRepository = LiveStreamRepositoryImpl(
    agoraService: agoraService,
  );
  Get.put<NewsRepository>(newsRepository);
  Get.put<LiveStreamRepository>(liveStreamRepository);
  Get.put<AgoraService>(agoraService);
  Get.put<NewsController>(NewsController(newsRepository: newsRepository));
  Get.put<LiveStreamController>(
    LiveStreamController(
      liveStreamRepository: liveStreamRepository,
      agoraService: agoraService,
    ),
  );
}

class FusionNewsApp extends StatelessWidget {
  const FusionNewsApp({super.key});

  @override
  Widget build(BuildContext context) {
    return GetMaterialApp(
      title: AppConstants.appName,
      debugShowCheckedModeBanner: false,
      theme: _buildTheme(),
      initialRoute: AppRoutes.home,
      getPages: _getPages(),
      defaultTransition: Transition.cupertino,
      transitionDuration: AppConstants.mediumAnimation,
      builder: (context, child) {
        return MediaQuery(
          data: MediaQuery.of(
            context,
          ).copyWith(textScaler: TextScaler.linear(1.2)),
          child: child!,
        );
      },
    );
  }

  ThemeData _buildTheme() {
    return ThemeData(
      useMaterial3: true,
      brightness: Brightness.dark,
      colorScheme: ColorScheme.fromSeed(
        seedColor: Colors.red,
        brightness: Brightness.dark,
      ),
      scaffoldBackgroundColor: Colors.black,
      appBarTheme: const AppBarTheme(
        backgroundColor: Colors.transparent,
        elevation: 0,
        iconTheme: IconThemeData(color: Colors.white),
        titleTextStyle: TextStyle(
          color: Colors.white,
          fontSize: 20,
          fontWeight: FontWeight.bold,
        ),
      ),
      textTheme: const TextTheme(
        bodyLarge: TextStyle(color: Colors.white),
        bodyMedium: TextStyle(color: Colors.white),
        bodySmall: TextStyle(color: Colors.white70),
        headlineLarge: TextStyle(color: Colors.white),
        headlineMedium: TextStyle(color: Colors.white),
        headlineSmall: TextStyle(color: Colors.white),
        titleLarge: TextStyle(color: Colors.white),
        titleMedium: TextStyle(color: Colors.white),
        titleSmall: TextStyle(color: Colors.white),
      ),
      elevatedButtonTheme: ElevatedButtonThemeData(
        style: ElevatedButton.styleFrom(
          backgroundColor: Colors.red,
          foregroundColor: Colors.white,
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(12),
          ),
        ),
      ),
    );
  }

  List<GetPage> _getPages() {
    return [
      GetPage(
        name: AppRoutes.home,
        page: () => const HomePage(),
        transition: Transition.fadeIn,
        transitionDuration: const Duration(milliseconds: 300),
      ),
      GetPage(
        name: AppRoutes.newsReel,
        page: () => const NewsReelPage(),
        transition: Transition.rightToLeft,
        transitionDuration: const Duration(milliseconds: 400),
      ),
      GetPage(
        name: AppRoutes.newsDetail,
        page: () {
          final article = Get.arguments as NewsArticle;
          return NewsDetailPage(article: article);
        },
        transition: Transition.rightToLeft,
        transitionDuration: const Duration(milliseconds: 400),
      ),
      GetPage(
        name: AppRoutes.liveStream,
        page: () => const LiveStreamPage(channelName: 'fusion_news_live'),
        transition: Transition.downToUp,
        transitionDuration: const Duration(milliseconds: 500),
      ),
      GetPage(
        name: AppRoutes.startLiveStream,
        page: () => const LiveStreamPage(
          isBroadcasting: true,
          channelName: 'fusion_news_live',
        ),
        transition: Transition.downToUp,
        transitionDuration: const Duration(milliseconds: 500),
      ),
    ];
  }
}
