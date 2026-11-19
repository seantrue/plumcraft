// iPadOS host app. Launch arguments as the Mac app; a relative --bench path lands in Documents.
import CanvasKit
import UIKit

@main
final class AppDelegate: UIResponder, UIApplicationDelegate {
    func application(_ app: UIApplication, configurationForConnecting session: UISceneSession, options: UIScene.ConnectionOptions) -> UISceneConfiguration {
        let c = UISceneConfiguration(name: "Default", sessionRole: session.role)
        c.delegateClass = SceneDelegate.self
        return c
    }
}

final class SceneDelegate: UIResponder, UIWindowSceneDelegate {
    var window: UIWindow?

    func scene(_ scene: UIScene, willConnectTo session: UISceneSession, options: UIScene.ConnectionOptions) {
        guard let ws = scene as? UIWindowScene else { return }
        let w = UIWindow(windowScene: ws)
        let vc = UIViewController()
        let screen = ws.screen
        let view = CanvasHostView(
            frame: ws.coordinateSpace.bounds, options: BenchOptions.parse(CommandLine.arguments),
            screenScale: screen.scale, maxFPS: screen.maximumFramesPerSecond)
        view.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        vc.view = view
        w.rootViewController = vc
        w.makeKeyAndVisible()
        window = w
        print("screen: scale \(screen.scale), max fps \(screen.maximumFramesPerSecond)")
    }
}
