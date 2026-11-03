trigger GreetingTrigger on Account (before insert) {
    System.debug('hello');
}
