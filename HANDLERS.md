# Implemented RPC dispatch inventory

Method routing against the pinned API layers 229 and 231. See README.md for behavior
and limitations; this inventory does not claim client compatibility testing.

## account

- `account.checkUsername`
- `account.getNotifySettings`
- `account.registerDevice`
- `account.reorderUsernames`
- `account.toggleUsername`
- `account.unregisterDevice`
- `account.updateProfile`
- `account.updateStatus`
- `account.updateUsername`

## auth

- `auth.bindTempAuthKey`
- `auth.logOut`
- `auth.sendCode`
- `auth.signIn`

## channels

- `channels.checkSearchPostsFlood`
- `channels.checkUsername`
- `channels.convertToGigagroup`
- `channels.createChannel`
- `channels.deactivateAllUsernames`
- `channels.deleteChannel`
- `channels.deleteHistory`
- `channels.deleteMessages`
- `channels.deleteParticipantHistory`
- `channels.editAdmin`
- `channels.editBanned`
- `channels.editLocation`
- `channels.editPhoto`
- `channels.editTitle`
- `channels.exportMessageLink`
- `channels.getAdminLog`
- `channels.getAdminedPublicChannels`
- `channels.getChannelRecommendations`
- `channels.getChannels`
- `channels.getFullChannel`
- `channels.getGroupsForDiscussion`
- `channels.getInactiveChannels`
- `channels.getLeftChannels`
- `channels.getMessageAuthor`
- `channels.getMessages`
- `channels.getParticipant`
- `channels.getParticipants`
- `channels.getSendAs`
- `channels.inviteToChannel`
- `channels.joinChannel`
- `channels.leaveChannel`
- `channels.readHistory`
- `channels.readMessageContents`
- `channels.reorderUsernames`
- `channels.reportAntiSpamFalsePositive`
- `channels.reportSpam`
- `channels.restrictSponsoredMessages`
- `channels.searchPosts`
- `channels.setBoostsToUnblockRestrictions`
- `channels.setDiscussionGroup`
- `channels.setEmojiStickers`
- `channels.setMainProfileTab`
- `channels.setStickers`
- `channels.toggleAntiSpam`
- `channels.toggleAutotranslation`
- `channels.toggleForum`
- `channels.toggleJoinRequest`
- `channels.toggleJoinToSend`
- `channels.toggleParticipantsHidden`
- `channels.togglePreHistoryHidden`
- `channels.toggleSignatures`
- `channels.toggleSlowMode`
- `channels.toggleUsername`
- `channels.toggleViewForumAsMessages`
- `channels.updateColor`
- `channels.updateEmojiStatus`
- `channels.updatePaidMessagesPrice`
- `channels.updateUsername`

## contacts

- `contacts.getContacts`
- `contacts.resolveUsername`
- `contacts.search`

## fragment

- `fragment.getCollectibleInfo`

## help

- `help.getAppConfig`
- `help.getConfig`
- `help.getNearestDc`
- `help.getPremiumPromo`
- `help.getTermsOfServiceUpdate`

## messages

- `messages.addChatUser`
- `messages.checkChatInvite`
- `messages.createChat`
- `messages.createForumTopic`
- `messages.deleteExportedChatInvite`
- `messages.deleteHistory`
- `messages.deleteMessages`
- `messages.deleteRevokedExportedChatInvites`
- `messages.deleteTopicHistory`
- `messages.editChatAbout`
- `messages.editChatCreator`
- `messages.editChatDefaultBannedRights`
- `messages.editChatPhoto`
- `messages.editChatTitle`
- `messages.editExportedChatInvite`
- `messages.editForumTopic`
- `messages.editMessage`
- `messages.exportChatInvite`
- `messages.forwardMessages`
- `messages.getAllDrafts`
- `messages.getChatInviteImporters`
- `messages.getChats`
- `messages.getDialogFilters`
- `messages.getDialogs`
- `messages.getExportedChatInvites`
- `messages.getForumTopics`
- `messages.getForumTopicsByID`
- `messages.getFullChat`
- `messages.getHistory`
- `messages.getMessagesViews`
- `messages.getPeerDialogs`
- `messages.getPeerSettings`
- `messages.getPinnedDialogs`
- `messages.getReplies`
- `messages.getScheduledHistory`
- `messages.hideAllChatJoinRequests`
- `messages.hideChatJoinRequest`
- `messages.importChatInvite`
- `messages.readHistory`
- `messages.readMentions`
- `messages.readReactions`
- `messages.reorderPinnedForumTopics`
- `messages.search`
- `messages.searchGlobal`
- `messages.sendMedia`
- `messages.sendMessage`
- `messages.setTyping`
- `messages.toggleNoForwards`
- `messages.unpinAllMessages`
- `messages.updatePinnedForumTopic`
- `messages.updatePinnedMessage`
- `messages.uploadMedia`

## payments

- `payments.checkCanSendGift`
- `payments.convertStarGift`
- `payments.craftStarGift`
- `payments.getCraftStarGifts`
- `payments.getPaymentForm`
- `payments.getPremiumGiftCodeOptions`
- `payments.getResaleStarGifts`
- `payments.getSavedStarGift`
- `payments.getSavedStarGifts`
- `payments.getStarGiftActiveAuctions`
- `payments.getStarGiftAuctionAcquiredGifts`
- `payments.getStarGiftAuctionState`
- `payments.getStarGiftUpgradeAttributes`
- `payments.getStarGiftUpgradePreview`
- `payments.getStarGifts`
- `payments.getStarsGiftOptions`
- `payments.getStarsGiveawayOptions`
- `payments.getStarsStatus`
- `payments.getStarsSubscriptions`
- `payments.getStarsTopupOptions`
- `payments.getStarsTransactions`
- `payments.getStarsTransactionsByID`
- `payments.getUniqueStarGift`
- `payments.saveStarGift`
- `payments.sendStarsForm`
- `payments.toggleStarGiftsPinnedToTop`
- `payments.transferStarGift`
- `payments.updateStarGiftPrice`
- `payments.upgradeStarGift`

## updates

- `updates.getChannelDifference`
- `updates.getDifference`
- `updates.getState`

## upload

- `upload.getFile`
- `upload.saveBigFilePart`
- `upload.saveFilePart`

## users

- `users.getFullUser`
- `users.getUsers`
